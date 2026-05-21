use std::collections::HashMap;

use nom::{
    bytes::complete::{tag, take_until, take_until1, take_while, take_while1},
    character::complete::{char, digit1},
    combinator::{map_res, opt},
    multi::many0,
    sequence::{preceded, terminated},
    Parser,
};
use uuid::{uuid, Uuid};

const VS_SOLUTION_FOLDER: Uuid = uuid!("2150E333-8FDC-42A3-9474-1A3956D46DE8");
const VS_CPP_PROJECT: Uuid = uuid!("8BC9CEB8-8B4A-11D0-8D11-00A0C91BC942");
const VS_CSHARP_PROJECT: Uuid = uuid!("FAE04EC0-301F-11D3-BF4B-00C04F79EFBC");

#[derive(Debug)]
pub struct Sln {
    pub projects: Vec<Project>,
    pub global: Global,
}

#[derive(Debug)]
pub struct Project {
    pub kind: ProjectKind,
    pub name: String,
    pub path: String,
    pub uuid: Uuid,

    pub section_dependencies: Option<SectionDependencies>,
}

#[derive(PartialEq, Debug, Eq)]
pub enum ProjectKind {
    Folder,
    Cpp,
    CSharp,
}

#[derive(Debug)]
pub struct SectionDependencies {
    pub deps: Vec<SectionDependency>,
}

#[derive(Debug)]
pub struct SectionDependency {
    pub uuid: Uuid,
}

#[derive(Debug)]
pub struct Global {
    pub sln_platforms: SolutionConfigurationPlatforms, // pre
    pub cfg_platforms: ProjectConfigurationPlatforms,  // post
    pub sln_properties: SolutionProperties,            // pre
    pub nested_projects: NestedProjects,               // pre
}

#[derive(Debug)]
pub struct SolutionConfigurationPlatforms {
    pub platforms: Vec<SolutionConfigurationPlatform>,
}

#[derive(Debug)]
pub struct SolutionConfigurationPlatform {
    pub platform: ConfigurationPlatform,
}

#[derive(Debug)]
pub struct ProjectConfigurationPlatforms {
    pub platforms: Vec<ProjectConfigurationPlatform>,
}

#[derive(Debug)]
pub struct ProjectConfigurationPlatform {
    pub uuid: Uuid,
    pub target_cfg: ConfigurationPlatform,
    pub actual_cfg: ConfigurationPlatform,
    pub is_enabled: bool,
}

#[derive(Debug)]
pub struct SolutionProperties {
    pub hide_solution_node: bool,
}

#[derive(Debug)]
pub struct NestedProjects {
    pub projects: Vec<NestedProject>,
}

#[derive(Debug)]
pub struct NestedProject {
    pub from: Uuid,
    pub to: Uuid,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigurationPlatform(pub String);

impl Sln {
    pub fn find_project_dependencies(&self, project_name: &str) -> Option<Vec<&Project>> {
        let mut result = vec![];

        let mut projects = self
            .projects
            .iter()
            .map(|project| (project.uuid, project))
            .collect::<HashMap<_, _>>();

        let project_uuid = self
            .projects
            .iter()
            .find(|project| project_name == project.name)?
            .uuid;

        let mut project_stack = vec![project_uuid];

        while let Some(project_uuid) = project_stack.pop() {
            let Some(project) = projects.remove(&project_uuid) else {
                // This assumes project is well-formed.
                continue;
            };

            result.push(project);

            if let Some(section_dependencies) = &project.section_dependencies {
                for section_dependency in &section_dependencies.deps {
                    project_stack.push(section_dependency.uuid);
                }
            }
        }

        // From least-specific to most-specific one
        result.reverse();

        Some(result)
    }
}

//
// Parsing
//

impl Sln {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = opt(tag("\u{FEFF}")).parse(i)?;

        let (i, _) = sp(i)?;
        let (i, (major_version, minor_version)) = sln_version(i)?;
        if major_version != 10 && minor_version != 0 {
            panic!("Unknown version: {major_version}.{minor_version}");
        }

        let (i, _) = sp(i)?;
        let (i, vs_version) = vs_version(i)?;
        if vs_version != 2008 {
            panic!("Unknown VS version: {vs_version}");
        }

        let (i, _) = sp(i)?;
        let (i, projects) = many0(Project::parse).parse(i)?;

        let (i, _) = sp(i)?;
        let (i, global) = Global::parse(i)?;

        Ok((i, Self { projects, global }))
    }
}

impl Project {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = tag("Project").parse(i)?;

        let (i, project_uuid) = parse_parentheses(i)?;
        let (j, project_uuid) = parse_uuid(project_uuid)?;
        assert_eq!(j, "");

        // TODO: Its own function + better error handling
        let kind = match () {
            () if project_uuid == VS_SOLUTION_FOLDER => ProjectKind::Folder,
            () if project_uuid == VS_CPP_PROJECT => ProjectKind::Cpp,
            () if project_uuid == VS_CSHARP_PROJECT => ProjectKind::CSharp,
            () => panic!("Unknown project uuid: {project_uuid}"),
        };

        let (i, _) = charsep('=').parse(i)?;

        let (i, name) = parse_string(i)?;
        let (i, _) = charsep(',').parse(i)?;

        let (i, path) = parse_string(i)?;
        let (i, _) = charsep(',').parse(i)?;

        let (i, uuid) = parse_uuid(i)?;
        let (i, _) = sp(i)?;

        let (i, section_dependencies) = opt(SectionDependencies::parse).parse(i)?;

        let (i, _) = tag("EndProject").parse(i)?;
        let (i, _) = sp(i)?;

        Ok((
            i,
            Self {
                kind,
                name: name.to_string(),
                path: path.to_string(),
                uuid,
                section_dependencies,
            },
        ))
    }
}

impl SectionDependencies {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = tag("ProjectSection(ProjectDependencies) = postProject").parse(i)?;
        let (i, _) = sp(i)?;

        let (i, deps) = many0(SectionDependency::parse).parse(i)?;

        let (i, _) = tag("EndProjectSection").parse(i)?;
        let (i, _) = sp(i)?;

        Ok((i, Self { deps }))
    }
}

impl SectionDependency {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, from) = parse_uuid_raw(i)?;
        let (i, _) = charsep('=').parse(i)?;

        let (i, to) = parse_uuid_raw(i)?;
        let (i, _) = sp(i)?;
        assert_eq!(from, to);

        Ok((i, Self { uuid: from }))
    }
}

impl Global {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = tag("Global").parse(i)?;
        let (i, _) = sp(i)?;

        let (i, sln_platforms) = SolutionConfigurationPlatforms::parse(i)?;
        let (i, cfg_platforms) = ProjectConfigurationPlatforms::parse(i)?;
        let (i, sln_properties) = SolutionProperties::parse(i)?;
        let (i, nested_projects) = NestedProjects::parse(i)?;

        let (i, _) = tag("EndGlobal").parse(i)?;
        let (i, _) = sp(i)?;

        Ok((
            i,
            Self {
                sln_platforms,
                cfg_platforms,
                sln_properties,
                nested_projects,
            },
        ))
    }
}

impl SolutionConfigurationPlatforms {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = tag("GlobalSection(SolutionConfigurationPlatforms) = preSolution").parse(i)?;
        let (i, _) = sp(i)?;

        let (i, platforms) = many0(SolutionConfigurationPlatform::parse).parse(i)?;

        let (i, _) = tag("EndGlobalSection").parse(i)?;
        let (i, _) = sp(i)?;

        Ok((i, Self { platforms }))
    }
}

impl SolutionConfigurationPlatform {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, platform_lhs) = ConfigurationPlatform::parse(i)?;
        let (i, _) = charsep('=').parse(i)?;
        let (i, platform_rhs) = ConfigurationPlatform::parse(i)?;
        let (i, _) = sp(i)?;

        assert_eq!(platform_lhs, platform_rhs);

        Ok((
            i,
            Self {
                platform: platform_lhs,
            },
        ))
    }
}

impl ProjectConfigurationPlatforms {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = tag("GlobalSection(ProjectConfigurationPlatforms) = postSolution").parse(i)?;
        let (i, _) = sp(i)?;

        let (i, platforms) = many0(ProjectConfigurationPlatform::parse).parse(i)?;

        let (i, _) = tag("EndGlobalSection").parse(i)?;
        let (i, _) = sp(i)?;

        Ok((i, Self { platforms }))
    }
}

impl ProjectConfigurationPlatform {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, uuid) = parse_uuid_raw(i)?;
        let (i, _) = char('.').parse(i)?;
        let (i, target_cfg) = ConfigurationPlatform::parse(i)?;
        let (i, _) = char('.').parse(i)?;
        let (i, _) = tag("ActiveCfg").parse(i)?;
        let (i, _) = charsep('=').parse(i)?;
        let (i, actual_cfg) = ConfigurationPlatform::parse(i)?;
        let (i, _) = sp(i)?;

        let mut this = Self {
            uuid,
            target_cfg,
            actual_cfg,
            is_enabled: false,
        };

        let (i, is_enabled) = opt(this.is_enabled_parser()).parse(i)?;
        this.is_enabled = is_enabled.is_some();

        Ok((i, this))
    }

    fn is_enabled_parser<'a>(&'a self) -> impl FnMut(&str) -> nom::IResult<&str, ()> + 'a {
        |i: &str| {
            let (i, uuid) = parse_uuid_raw(i)?;
            let (i, _) = char('.').parse(i)?;
            let (i, target_cfg) = ConfigurationPlatform::parse(i)?;
            let (i, _) = char('.').parse(i)?;
            let (i, _) = tag("Build.0").parse(i)?; // !!!
            let (i, _) = charsep('=').parse(i)?;
            let (i, cfg) = ConfigurationPlatform::parse(i)?;
            let (i, _) = sp(i)?;

            assert_eq!(self.uuid, uuid);
            assert_eq!(self.target_cfg, target_cfg);
            assert_eq!(self.actual_cfg, cfg);

            Ok((i, ()))
        }
    }
}
impl ConfigurationPlatform {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, build_kind) = take_until1("|").parse(i)?;
        let (i, _) = char('|').parse(i)?;
        let (i, platform_name) = Self::take_until1_platform_sep(i)?;
        let (i, _) = sp(i)?;

        Ok((i, Self(format!("{build_kind}|{platform_name}"))))
    }

    fn take_until1_platform_sep(i: &str) -> nom::IResult<&str, &str> {
        take_while1(move |c| !" .\t\r\n".contains(c)).parse(i)
    }

    pub fn configuration_n_platform(&self) -> (&str, &str) {
        self.0.split_once('|').unwrap()
    }
}

impl SolutionProperties {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = tag("GlobalSection(SolutionProperties) = preSolution").parse(i)?;
        let (i, _) = sp(i)?;
        let (i, _) = tag("HideSolutionNode = FALSE").parse(i)?;
        let (i, _) = sp(i)?;
        let (i, _) = tag("EndGlobalSection").parse(i)?;
        let (i, _) = sp(i)?;
        Ok((
            i,
            Self {
                hide_solution_node: false,
            },
        ))
    }
}

impl NestedProjects {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, _) = tag("GlobalSection(NestedProjects) = preSolution").parse(i)?;
        let (i, _) = sp(i)?;

        let (i, projects) = many0(NestedProject::parse).parse(i)?;

        let (i, _) = tag("EndGlobalSection").parse(i)?;
        let (i, _) = sp(i)?;

        Ok((i, Self { projects }))
    }
}

impl NestedProject {
    pub fn parse(i: &str) -> nom::IResult<&str, Self> {
        let (i, from) = parse_uuid_raw(i)?;
        let (i, _) = charsep('=').parse(i)?;
        let (i, to) = parse_uuid_raw(i)?;
        let (i, _) = sp(i)?;

        Ok((i, Self { from, to }))
    }
}

fn parse_uuid(i: &str) -> nom::IResult<&str, Uuid> {
    let (i, up_uuid) = parse_string(i)?;
    let (j, up_uuid) = parse_uuid_raw(up_uuid)?;
    assert_eq!(j, "");

    Ok((i, up_uuid))
}

fn parse_uuid_raw(i: &str) -> nom::IResult<&str, Uuid> {
    let (i, up_uuid) = parse_curly(i)?;

    let up_uuid = Uuid::parse_str(up_uuid)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(i, nom::error::ErrorKind::Fail)))?;

    Ok((i, up_uuid))
}

fn parse_curly(i: &str) -> nom::IResult<&str, &str> {
    preceded(char('{'), terminated(take_until("}"), char('}'))).parse(i)
}

fn parse_parentheses(i: &str) -> nom::IResult<&str, &str> {
    preceded(char('('), terminated(take_until(")"), char(')'))).parse(i)
}

fn parse_string(i: &str) -> nom::IResult<&str, &str> {
    preceded(char('"'), terminated(take_until("\""), char('"'))).parse(i)
}

fn vs_version(i: &str) -> nom::IResult<&str, u16> {
    let (i, _) = tag("# Visual Studio ").parse(i)?;
    let (i, version) = map_res(digit1, str::parse::<u16>).parse(i)?;

    Ok((i, version))
}

fn sln_version(i: &str) -> nom::IResult<&str, (u8, u8)> {
    let (i, _) = tag("Microsoft Visual Studio Solution File, Format Version ").parse(i)?;
    let (i, major_version) = map_res(digit1, str::parse::<u8>).parse(i)?;
    let (i, _) = tag(".").parse(i)?;
    let (i, minor_version) = map_res(digit1, str::parse::<u8>).parse(i)?;

    Ok((i, (major_version, minor_version)))
}

fn charsep(sep: char) -> impl FnMut(&str) -> nom::IResult<&str, char> {
    move |i: &str| {
        let (i, _) = sp(i)?;
        let (i, sep) = char(sep).parse(i)?;
        let (i, _) = sp(i)?;

        Ok((i, sep))
    }
}

fn sp(i: &str) -> nom::IResult<&str, &str> {
    take_while(move |c| " \t\r\n".contains(c)).parse(i)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_sln_version() {
        let input = "Microsoft Visual Studio Solution File, Format Version 12.13";
        assert_eq!(sln_version(input).unwrap().1, (12, 13));
    }

    #[test]
    fn parses_escaped_str() {
        let input = "\"hello\"";
        assert_eq!(parse_string(input).unwrap().1, "hello");
    }

    #[test]
    fn parses_empty_project() {
        let input = r#"
Project("{2150E333-8FDC-42A3-9474-1A3956D46DE8}") = "survarium", "survarium", "{4E2399DA-D511-4F61-ACCB-894F87214FC5}"
EndProject
"#.trim();

        let project = Project::parse(input).unwrap().1;
        assert_eq!(project.name, "survarium");
        assert_eq!(project.path, "survarium");
        assert_eq!(project.kind, ProjectKind::Folder);
        assert_eq!(
            project.uuid,
            Uuid::parse_str("{4E2399DA-D511-4F61-ACCB-894F87214FC5}").unwrap()
        );
    }

    #[test]
    fn parse_project() {
        let input = r#"
Project("{8BC9CEB8-8B4A-11D0-8D11-00A0C91BC942}") = "bugtrap", "BugTrap\BugTrap.vcproj", "{E8CF1ADA-264A-4127-86C2-FD7057D3792C}"
	ProjectSection(ProjectDependencies) = postProject
		{CA2604CA-A2AC-49A1-A468-8AB5A2E6CBC9} = {CA2604CA-A2AC-49A1-A468-8AB5A2E6CBC9}
		{893279CB-0805-405F-B484-9BB728A18261} = {893279CB-0805-405F-B484-9BB728A18261}
	EndProjectSection
EndProject
"#.trim();

        let project = Project::parse(input).unwrap().1;
        assert_eq!(project.name, "bugtrap");
        assert_eq!(project.path, "BugTrap\\BugTrap.vcproj");
        assert_eq!(project.section_dependencies.unwrap().deps.len(), 2);
    }

    #[test]
    fn parse_configuration_platform() {
        let input = r#"
		Master Gold|Win32 = Master Gold|Win32
        "#
        .trim();
        let (i, conf) = SolutionConfigurationPlatform::parse(input).unwrap();

        assert_eq!(i, "");
        assert_eq!(conf.platform.0, "Master Gold|Win32");
        // assert_eq!(conf.platform.build_kind, "Master Gold");
        // assert_eq!(conf.platform.platform_name, "Win32");
    }

    #[test]
    fn parse_configuration_platforms() {
        let input = r#"
            GlobalSection(ProjectConfigurationPlatforms) = postSolution
            	{A0327856-D686-4659-90B9-226877A9D11F}.Master Gold|Win32.ActiveCfg = Master Gold|Win32
            	{A0327856-D686-4659-90B9-226877A9D11F}.Master Gold|Win32.Build.0 = Master Gold|Win32
            	{E7FF01A9-20EA-431D-8EE5-71631F8C05A5}.Master Gold|Win32.ActiveCfg = Master Gold|Win32
            	{E7FF01A9-20EA-431D-8EE5-71631F8C05A5}.Master Gold|Win32.ActiveCfg = Master Gold|Win32
            EndGlobalSection
        "#
        .trim();
        let (i, conf) = ProjectConfigurationPlatforms::parse(input).unwrap();

        assert_eq!(i, "");
        assert_eq!(conf.platforms.len(), 3);
        assert_eq!(conf.platforms[0].is_enabled, true);
        assert_eq!(conf.platforms[1].is_enabled, false);
        assert_eq!(conf.platforms[2].is_enabled, false);
    }

    #[test]
    fn parses_sln() {
        const SLN: &str = include_str!("../../../resources/vostok.sln");

        let (i, _sln) = Sln::parse(SLN).unwrap();
        assert_eq!(i, "");
    }
}
