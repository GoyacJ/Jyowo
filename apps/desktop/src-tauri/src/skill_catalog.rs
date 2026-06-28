use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::TempDir;
use zip::ZipArchive;

use crate::commands::CommandErrorPayload;

const ANTHROPIC_REPO_OWNER: &str = "anthropics";
const ANTHROPIC_REPO_NAME: &str = "skills";
const ANTHROPIC_REPO_REF: &str = "main";
const AWESOME_OWNER: &str = "VoltAgent";
const AWESOME_REPO: &str = "awesome-agent-skills";
const AWESOME_REPO_REF: &str = "main";
const CATALOG_USER_AGENT: &str = "Jyowo skill catalog";
const MAX_CATALOG_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_CATALOG_PACKAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_CATALOG_PACKAGE_FILE_BYTES: usize = 1024 * 1024;
const MAX_CATALOG_PACKAGE_FILES: usize = 200;
const MAX_CATALOG_PREVIEW_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ListSkillCatalogEntriesRequest {
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetSkillCatalogEntryRequest {
    pub source_id: String,
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstallSkillFromCatalogRequest {
    pub source_id: String,
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetSkillCatalogFileRequest {
    pub source_id: String,
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallOriginRecord {
    pub source_id: String,
    pub source_label: String,
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage_url: Option<String>,
    pub installed_from_catalog: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogSourcePayload {
    pub id: String,
    pub label: String,
    pub description: String,
    pub trust_level: String,
    pub installable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogEntryPayload {
    pub source_id: String,
    pub source_label: String,
    pub entry_id: String,
    pub name: String,
    pub description: String,
    pub trust_level: String,
    pub installable: bool,
    pub installed: bool,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogValidationPayload {
    pub status: String,
    pub issues: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issue_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogFilePayload {
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillCatalogSourcesResponse {
    pub sources: Vec<SkillCatalogSourcePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillCatalogEntriesResponse {
    pub entries: Vec<SkillCatalogEntryPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSkillCatalogEntryResponse {
    pub entry: SkillCatalogEntryPayload,
    pub validation: SkillCatalogValidationPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<SkillCatalogFilePayload>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogFileContentPayload {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSkillCatalogFileResponse {
    pub file: SkillCatalogFileContentPayload,
}

pub struct MaterializedCatalogSkill {
    pub temp_dir: TempDir,
    pub package_path: PathBuf,
    pub origin: SkillInstallOriginRecord,
}

pub type CatalogInstallProgressSink<'a> = &'a (dyn Fn(&str, u8) + Send + Sync);

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubSkillRef {
    owner: String,
    repo: String,
    reference: String,
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubTreeResponse {
    tree: Vec<GithubTreeItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubTreeItem {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubCommitResponse {
    sha: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClawHubListResponse {
    #[serde(default, alias = "results")]
    items: Vec<ClawHubSkillItem>,
    #[serde(default, rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClawHubSkillItem {
    slug: String,
    #[serde(default, rename = "ownerHandle")]
    owner_handle: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "latestVersion")]
    latest_version: Option<ClawHubVersionSummary>,
}

#[derive(Debug, Clone)]
struct ClawHubDetailResponse {
    skill: ClawHubSkillItem,
}

impl<'de> Deserialize<'de> for ClawHubDetailResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wrapped {
            skill: ClawHubSkillItem,
        }

        let value = Value::deserialize(deserializer)?;
        if value.get("skill").is_some() {
            let wrapped = Wrapped::deserialize(value).map_err(serde::de::Error::custom)?;
            Ok(Self {
                skill: wrapped.skill,
            })
        } else {
            let skill = ClawHubSkillItem::deserialize(value).map_err(serde::de::Error::custom)?;
            Ok(Self { skill })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClawHubEntryKey {
    owner_handle: Option<String>,
    slug: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClawHubVersionSummary {
    version: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClawHubScanResponse {
    #[serde(default)]
    security: Option<Value>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubGithubHandoff {
    source_ref: String,
    repo: String,
    commit: String,
    path: String,
    archive_url: String,
}

pub fn fixed_catalog_sources() -> Vec<SkillCatalogSourcePayload> {
    vec![
        SkillCatalogSourcePayload {
            id: "anthropic".to_owned(),
            label: "Anthropic Skills".to_owned(),
            description: "Official Anthropic skills repository.".to_owned(),
            trust_level: "official".to_owned(),
            installable: true,
        },
        SkillCatalogSourcePayload {
            id: "agent-skills-spec".to_owned(),
            label: "Agent Skills spec".to_owned(),
            description: "Validation standard for portable agent skills.".to_owned(),
            trust_level: "standard".to_owned(),
            installable: false,
        },
        SkillCatalogSourcePayload {
            id: "awesome-agent-skills".to_owned(),
            label: "Awesome Agent Skills".to_owned(),
            description: "Curated community index of agent skill repositories.".to_owned(),
            trust_level: "curated".to_owned(),
            installable: true,
        },
        SkillCatalogSourcePayload {
            id: "clawhub".to_owned(),
            label: "ClawHub".to_owned(),
            description: "Public ClawHub registry with security scan metadata.".to_owned(),
            trust_level: "community".to_owned(),
            installable: true,
        },
    ]
}

pub fn list_skill_catalog_sources() -> ListSkillCatalogSourcesResponse {
    ListSkillCatalogSourcesResponse {
        sources: fixed_catalog_sources(),
    }
}

pub async fn list_skill_catalog_entries(
    request: ListSkillCatalogEntriesRequest,
    installed_entry_ids: &HashSet<String>,
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload> {
    ensure_catalog_source(&request.source_id)?;
    match request.source_id.as_str() {
        "anthropic" => list_anthropic_entries(&request, installed_entry_ids).await,
        "agent-skills-spec" => Ok(paginate_catalog_entries(
            list_agent_skill_spec_entry(installed_entry_ids).entries,
            request.cursor.as_deref(),
            request.limit,
        )),
        "awesome-agent-skills" => list_awesome_entries(&request, installed_entry_ids).await,
        "clawhub" => list_clawhub_entries(&request, installed_entry_ids).await,
        _ => Err(invalid_payload("unknown skill catalog source".to_owned())),
    }
}

pub async fn get_skill_catalog_entry(
    request: GetSkillCatalogEntryRequest,
    installed_entry_ids: &HashSet<String>,
) -> Result<GetSkillCatalogEntryResponse, CommandErrorPayload> {
    ensure_catalog_source(&request.source_id)?;
    let response = match request.source_id.as_str() {
        "anthropic" => {
            get_github_catalog_entry(
                &request,
                GithubSkillRef {
                    owner: ANTHROPIC_REPO_OWNER.to_owned(),
                    repo: ANTHROPIC_REPO_NAME.to_owned(),
                    reference: request
                        .version
                        .clone()
                        .unwrap_or_else(|| ANTHROPIC_REPO_REF.to_owned()),
                    path: entry_tail(&request.entry_id, "anthropic:")?.to_owned(),
                },
                "Anthropic Skills",
                "official",
                installed_entry_ids,
                false,
            )
            .await?
        }
        "agent-skills-spec" => get_agent_skill_spec_entry(installed_entry_ids),
        "awesome-agent-skills" => {
            let github_ref = parse_awesome_entry_id(&request.entry_id)?;
            get_github_catalog_entry(
                &request,
                github_ref,
                "Awesome Agent Skills",
                "curated",
                installed_entry_ids,
                true,
            )
            .await?
        }
        "clawhub" => get_clawhub_entry(&request, installed_entry_ids).await?,
        _ => return Err(invalid_payload("unknown skill catalog source".to_owned())),
    };
    Ok(response)
}

pub async fn get_skill_catalog_file(
    request: GetSkillCatalogFileRequest,
) -> Result<GetSkillCatalogFileResponse, CommandErrorPayload> {
    ensure_catalog_source(&request.source_id)?;
    let path = ensure_catalog_file_path(&request.path)?;
    match request.source_id.as_str() {
        "anthropic" => {
            let reference = request
                .version
                .unwrap_or_else(|| ANTHROPIC_REPO_REF.to_owned());
            let skill_path = entry_tail(&request.entry_id, "anthropic:")?;
            let source_path = catalog_file_source_path(skill_path, &path);
            fetch_raw_github_file_preview(
                ANTHROPIC_REPO_OWNER,
                ANTHROPIC_REPO_NAME,
                &reference,
                &source_path,
                &path,
            )
            .await
        }
        "awesome-agent-skills" => {
            let github_ref = parse_awesome_entry_id(&request.entry_id)?;
            let source_path = catalog_file_source_path(&github_ref.path, &path);
            fetch_raw_github_file_preview(
                &github_ref.owner,
                &github_ref.repo,
                request.version.as_deref().unwrap_or(&github_ref.reference),
                &source_path,
                &path,
            )
            .await
        }
        "clawhub" => {
            let key = parse_clawhub_entry_id(&request.entry_id)?;
            let client = http_client()?;
            fetch_clawhub_file_preview(
                &client,
                &key.slug,
                key.owner_handle.as_deref(),
                request.version.as_deref(),
                &path,
            )
            .await
        }
        "agent-skills-spec" => {
            if path == "SKILL.md" {
                Ok(catalog_file_response(
                    &path,
                    "Requires frontmatter name and description.\nSkill name must be lowercase alphanumeric with hyphens.\n".to_owned(),
                    false,
                ))
            } else {
                Err(invalid_payload("catalog file not found".to_owned()))
            }
        }
        _ => Err(invalid_payload("unknown skill catalog source".to_owned())),
    }
}

pub async fn materialize_skill_from_catalog(
    request: InstallSkillFromCatalogRequest,
) -> Result<MaterializedCatalogSkill, CommandErrorPayload> {
    materialize_skill_from_catalog_with_progress(request, None).await
}

pub async fn materialize_skill_from_catalog_with_progress(
    request: InstallSkillFromCatalogRequest,
    progress: Option<CatalogInstallProgressSink<'_>>,
) -> Result<MaterializedCatalogSkill, CommandErrorPayload> {
    ensure_catalog_source(&request.source_id)?;
    match request.source_id.as_str() {
        "anthropic" => {
            materialize_github_skill(
                GithubSkillRef {
                    owner: ANTHROPIC_REPO_OWNER.to_owned(),
                    repo: ANTHROPIC_REPO_NAME.to_owned(),
                    reference: request
                        .version
                        .unwrap_or_else(|| ANTHROPIC_REPO_REF.to_owned()),
                    path: entry_tail(&request.entry_id, "anthropic:")?.to_owned(),
                },
                "anthropic",
                "Anthropic Skills",
                request.entry_id,
                Some("https://github.com/anthropics/skills".to_owned()),
                progress,
            )
            .await
        }
        "awesome-agent-skills" => {
            let github_ref = parse_awesome_entry_id(&request.entry_id)?;
            ensure_awesome_entry_allowed(&github_ref).await?;
            let homepage = Some(format!(
                "https://github.com/{}/{}",
                github_ref.owner, github_ref.repo
            ));
            materialize_github_skill(
                github_ref,
                "awesome-agent-skills",
                "Awesome Agent Skills",
                request.entry_id,
                homepage,
                progress,
            )
            .await
        }
        "clawhub" => materialize_clawhub_skill(request, progress).await,
        "agent-skills-spec" => Err(invalid_payload(
            "Agent Skills spec is a validation standard, not an install source".to_owned(),
        )),
        _ => Err(invalid_payload("unknown skill catalog source".to_owned())),
    }
}

pub fn clawhub_scan_allows_install(status: Option<&str>) -> bool {
    matches!(status, Some("clean"))
}

fn ensure_catalog_source(source_id: &str) -> Result<(), CommandErrorPayload> {
    if fixed_catalog_sources()
        .iter()
        .any(|source| source.id == source_id)
    {
        Ok(())
    } else {
        Err(invalid_payload("unknown skill catalog source".to_owned()))
    }
}

fn installed(installed_entry_ids: &HashSet<String>, entry_id: &str) -> bool {
    installed_entry_ids.contains(entry_id)
}

async fn list_anthropic_entries(
    request: &ListSkillCatalogEntriesRequest,
    installed_entry_ids: &HashSet<String>,
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload> {
    let tree = fetch_github_tree(
        ANTHROPIC_REPO_OWNER,
        ANTHROPIC_REPO_NAME,
        ANTHROPIC_REPO_REF,
    )
    .await?;
    let mut entries = Vec::new();
    for item in tree.tree.iter().filter(|item| item.kind == "blob") {
        let Some(dir) = anthropic_skill_dir_from_tree_path(&item.path) else {
            continue;
        };
        let entry_id = format!("anthropic:{dir}");
        let summary = fetch_github_skill_summary(
            ANTHROPIC_REPO_OWNER,
            ANTHROPIC_REPO_NAME,
            ANTHROPIC_REPO_REF,
            &dir,
        )
        .await
        .unwrap_or_else(|_| SkillFrontmatterSummary {
            name: dir.to_owned(),
            description: "Anthropic skill package.".to_owned(),
            tags: Vec::new(),
        });
        entries.push(SkillCatalogEntryPayload {
            source_id: "anthropic".to_owned(),
            source_label: "Anthropic Skills".to_owned(),
            entry_id: entry_id.clone(),
            name: summary.name,
            description: summary.description,
            trust_level: "official".to_owned(),
            installable: true,
            installed: installed(installed_entry_ids, &entry_id),
            tags: summary.tags,
            version: Some(ANTHROPIC_REPO_REF.to_owned()),
            homepage_url: Some(format!(
                "https://github.com/anthropics/skills/tree/main/{dir}"
            )),
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(paginate_catalog_entries(
        entries,
        request.cursor.as_deref(),
        request.limit,
    ))
}

fn list_agent_skill_spec_entry(
    installed_entry_ids: &HashSet<String>,
) -> ListSkillCatalogEntriesResponse {
    let entry_id = "agent-skills-spec:specification".to_owned();
    ListSkillCatalogEntriesResponse {
        entries: vec![SkillCatalogEntryPayload {
            source_id: "agent-skills-spec".to_owned(),
            source_label: "Agent Skills spec".to_owned(),
            entry_id: entry_id.clone(),
            name: "Agent Skills specification".to_owned(),
            description: "Portable skill format rules used to validate catalog installs."
                .to_owned(),
            trust_level: "standard".to_owned(),
            installable: false,
            installed: installed(installed_entry_ids, &entry_id),
            tags: vec!["specification".to_owned()],
            version: None,
            homepage_url: Some("https://agentskills.io/specification".to_owned()),
        }],
        next_cursor: None,
    }
}

fn get_agent_skill_spec_entry(
    installed_entry_ids: &HashSet<String>,
) -> GetSkillCatalogEntryResponse {
    let entry = list_agent_skill_spec_entry(installed_entry_ids)
        .entries
        .into_iter()
        .next()
        .expect("spec entry exists");
    GetSkillCatalogEntryResponse {
        entry,
        validation: SkillCatalogValidationPayload {
            status: "ready".to_owned(),
            issues: vec![
                "Requires SKILL.md at the skill root.".to_owned(),
                "Requires frontmatter name and description.".to_owned(),
                "Skill name must be lowercase alphanumeric with hyphens.".to_owned(),
            ],
            issue_codes: vec![
                "skill_root_required".to_owned(),
                "frontmatter_required".to_owned(),
                "skill_name_format".to_owned(),
            ],
        },
        readme_preview: Some(
            "This source is used as the validation standard for installed catalog skills."
                .to_owned(),
        ),
        files: None,
    }
}

async fn list_awesome_entries(
    request: &ListSkillCatalogEntriesRequest,
    installed_entry_ids: &HashSet<String>,
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload> {
    let readme =
        fetch_raw_github_file(AWESOME_OWNER, AWESOME_REPO, AWESOME_REPO_REF, "README.md").await?;
    let normalized_query = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(str::to_ascii_lowercase);
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for (label, url) in markdown_links(&readme) {
        let Some(github_ref) = parse_github_tree_url(&url) else {
            continue;
        };
        let entry_id = awesome_entry_id(&github_ref);
        if !seen.insert(entry_id.clone()) {
            continue;
        }
        let name = label.trim().trim_matches('`').to_owned();
        if let Some(query) = &normalized_query {
            let haystack = format!("{name} {url}").to_ascii_lowercase();
            if !haystack.contains(query) {
                continue;
            }
        }
        entries.push(SkillCatalogEntryPayload {
            source_id: "awesome-agent-skills".to_owned(),
            source_label: "Awesome Agent Skills".to_owned(),
            entry_id: entry_id.clone(),
            name: if name.is_empty() {
                github_ref.path.clone()
            } else {
                name
            },
            description: format!(
                "{}/{}: {}",
                github_ref.owner, github_ref.repo, github_ref.path
            ),
            trust_level: "curated".to_owned(),
            installable: true,
            installed: installed(installed_entry_ids, &entry_id),
            tags: vec!["community".to_owned()],
            version: Some(github_ref.reference),
            homepage_url: Some(url),
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(paginate_catalog_entries(
        entries,
        request.cursor.as_deref(),
        request.limit,
    ))
}

async fn list_clawhub_entries(
    request: &ListSkillCatalogEntriesRequest,
    installed_entry_ids: &HashSet<String>,
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload> {
    let client = http_client()?;
    let mut url = if let Some(query) = request
        .query
        .as_deref()
        .filter(|query| !query.trim().is_empty())
    {
        reqwest::Url::parse_with_params(
            "https://clawhub.ai/api/v1/search",
            [("q", query.trim()), ("nonSuspiciousOnly", "true")],
        )
    } else {
        let sort = request.sort.as_deref().unwrap_or("recommended");
        let limit = request.limit.unwrap_or(40).clamp(1, 100).to_string();
        reqwest::Url::parse_with_params(
            "https://clawhub.ai/api/v1/skills",
            [
                ("limit", limit.as_str()),
                ("sort", sort),
                ("nonSuspiciousOnly", "true"),
            ],
        )
    }
    .map_err(|error| runtime_operation_failed(format!("ClawHub URL build failed: {error}")))?;
    if let Some(cursor) = request
        .cursor
        .as_deref()
        .filter(|cursor| !cursor.trim().is_empty())
    {
        url.query_pairs_mut().append_pair("cursor", cursor);
    }
    let response =
        client.get(url).send().await.map_err(|error| {
            runtime_operation_failed(format!("ClawHub request failed: {error}"))
        })?;
    let response = ensure_success(response).await?;
    let payload = response
        .json::<ClawHubListResponse>()
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("ClawHub response parse failed: {error}"))
        })?;
    let entries = payload
        .items
        .into_iter()
        .map(|item| {
            let version = item.version.clone().or_else(|| {
                item.latest_version
                    .as_ref()
                    .map(|latest| latest.version.clone())
            });
            let entry_id = clawhub_entry_id(&item);
            SkillCatalogEntryPayload {
                source_id: "clawhub".to_owned(),
                source_label: "ClawHub".to_owned(),
                entry_id: entry_id.clone(),
                name: item.display_name.unwrap_or_else(|| item.slug.clone()),
                description: item
                    .summary
                    .or(item.description)
                    .unwrap_or_else(|| "ClawHub skill.".to_owned()),
                trust_level: "community".to_owned(),
                installable: true,
                installed: installed(installed_entry_ids, &entry_id),
                tags: item.topics,
                version,
                homepage_url: Some(clawhub_homepage_url(
                    &item.slug,
                    item.owner_handle.as_deref(),
                )),
            }
        })
        .collect();
    Ok(ListSkillCatalogEntriesResponse {
        entries,
        next_cursor: payload.next_cursor,
    })
}

async fn get_github_catalog_entry(
    request: &GetSkillCatalogEntryRequest,
    github_ref: GithubSkillRef,
    source_label: &str,
    trust_level: &str,
    installed_entry_ids: &HashSet<String>,
    block_missing_skill: bool,
) -> Result<GetSkillCatalogEntryResponse, CommandErrorPayload> {
    let skill_markdown = match fetch_raw_github_file(
        &github_ref.owner,
        &github_ref.repo,
        &github_ref.reference,
        &format!("{}/SKILL.md", github_ref.path),
    )
    .await
    {
        Ok(markdown) => markdown,
        Err(_error) if block_missing_skill => {
            return Ok(blocked_github_catalog_entry(
                request,
                github_ref,
                source_label,
                trust_level,
                installed_entry_ids,
                "SKILL.md is not readable from this catalog entry.",
                "skill_file_unreadable",
            )
            .await);
        }
        Err(error) => return Err(error),
    };
    let summary = parse_skill_frontmatter_summary(&skill_markdown).unwrap_or_else(|| {
        SkillFrontmatterSummary {
            name: github_ref.path.clone(),
            description: "GitHub skill package.".to_owned(),
            tags: Vec::new(),
        }
    });
    let files = github_catalog_files(&github_ref).await?;
    Ok(GetSkillCatalogEntryResponse {
        entry: SkillCatalogEntryPayload {
            source_id: request.source_id.clone(),
            source_label: source_label.to_owned(),
            entry_id: request.entry_id.clone(),
            name: summary.name,
            description: summary.description,
            trust_level: trust_level.to_owned(),
            installable: true,
            installed: installed(installed_entry_ids, &request.entry_id),
            tags: summary.tags,
            version: Some(github_ref.reference.clone()),
            homepage_url: Some(format!(
                "https://github.com/{}/{}/tree/{}/{}",
                github_ref.owner, github_ref.repo, github_ref.reference, github_ref.path
            )),
        },
        validation: validate_catalog_markdown(&skill_markdown),
        readme_preview: Some(skill_markdown.chars().take(2_000).collect()),
        files: Some(files),
    })
}

async fn blocked_github_catalog_entry(
    request: &GetSkillCatalogEntryRequest,
    github_ref: GithubSkillRef,
    source_label: &str,
    trust_level: &str,
    installed_entry_ids: &HashSet<String>,
    issue: &str,
    issue_code: &str,
) -> GetSkillCatalogEntryResponse {
    let files = github_catalog_files(&github_ref).await.ok();
    GetSkillCatalogEntryResponse {
        entry: SkillCatalogEntryPayload {
            source_id: request.source_id.clone(),
            source_label: source_label.to_owned(),
            entry_id: request.entry_id.clone(),
            name: github_ref
                .path
                .rsplit('/')
                .next()
                .filter(|value| !value.is_empty())
                .unwrap_or(github_ref.path.as_str())
                .to_owned(),
            description: format!(
                "{}/{}: {}",
                github_ref.owner, github_ref.repo, github_ref.path
            ),
            trust_level: trust_level.to_owned(),
            installable: false,
            installed: installed(installed_entry_ids, &request.entry_id),
            tags: Vec::new(),
            version: Some(github_ref.reference.clone()),
            homepage_url: Some(format!(
                "https://github.com/{}/{}/tree/{}/{}",
                github_ref.owner, github_ref.repo, github_ref.reference, github_ref.path
            )),
        },
        validation: SkillCatalogValidationPayload {
            status: "blocked".to_owned(),
            issues: vec![issue.to_owned()],
            issue_codes: vec![issue_code.to_owned()],
        },
        readme_preview: None,
        files,
    }
}

async fn github_catalog_files(
    github_ref: &GithubSkillRef,
) -> Result<Vec<SkillCatalogFilePayload>, CommandErrorPayload> {
    Ok(
        fetch_github_tree(&github_ref.owner, &github_ref.repo, &github_ref.reference)
            .await?
            .tree
            .into_iter()
            .filter_map(|item| {
                let relative = item.path.strip_prefix(&format!("{}/", github_ref.path))?;
                Some(SkillCatalogFilePayload {
                    path: relative.to_owned(),
                    kind: if item.kind == "tree" {
                        "directory"
                    } else {
                        "file"
                    }
                    .to_owned(),
                    size_bytes: item.size,
                })
            })
            .collect::<Vec<_>>(),
    )
}

async fn get_clawhub_entry(
    request: &GetSkillCatalogEntryRequest,
    installed_entry_ids: &HashSet<String>,
) -> Result<GetSkillCatalogEntryResponse, CommandErrorPayload> {
    let key = parse_clawhub_entry_id(&request.entry_id)?;
    let client = http_client()?;
    let mut detail_url =
        reqwest::Url::parse(&format!("https://clawhub.ai/api/v1/skills/{}", key.slug)).map_err(
            |error| runtime_operation_failed(format!("ClawHub detail URL build failed: {error}")),
        )?;
    append_owner_handle(&mut detail_url, key.owner_handle.as_deref());
    let detail = ensure_success(client.get(detail_url).send().await.map_err(|error| {
        runtime_operation_failed(format!("ClawHub detail request failed: {error}"))
    })?)
    .await?
    .json::<ClawHubDetailResponse>()
    .await
    .map_err(|error| runtime_operation_failed(format!("ClawHub detail parse failed: {error}")))?
    .skill;
    let version = request.version.clone().or_else(|| {
        detail.version.clone().or_else(|| {
            detail
                .latest_version
                .as_ref()
                .map(|latest| latest.version.clone())
        })
    });
    let owner_handle = key
        .owner_handle
        .as_deref()
        .or(detail.owner_handle.as_deref());
    let scan_status =
        fetch_clawhub_scan_status(&client, &key.slug, owner_handle, version.as_deref()).await?;
    let mut issues = Vec::new();
    let mut issue_codes = Vec::new();
    if !clawhub_scan_allows_install(scan_status.as_deref()) {
        issues.push("ClawHub scan is not clean.".to_owned());
        issue_codes.push("clawhub_scan_not_clean".to_owned());
    }
    let markdown = fetch_clawhub_file(
        &client,
        &key.slug,
        owner_handle,
        version.as_deref(),
        "SKILL.md",
    )
    .await
    .ok();
    let validation = markdown
        .as_deref()
        .map(validate_catalog_markdown)
        .unwrap_or_else(|| SkillCatalogValidationPayload {
            status: "blocked".to_owned(),
            issues: vec!["SKILL.md is not readable from ClawHub.".to_owned()],
            issue_codes: vec!["skill_file_unreadable".to_owned()],
        });
    issues.extend(validation.issues.clone());
    issue_codes.extend(validation.issue_codes.clone());
    let status = if issues.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    Ok(GetSkillCatalogEntryResponse {
        entry: SkillCatalogEntryPayload {
            source_id: "clawhub".to_owned(),
            source_label: "ClawHub".to_owned(),
            entry_id: request.entry_id.clone(),
            name: detail.display_name.unwrap_or_else(|| detail.slug.clone()),
            description: detail
                .summary
                .or(detail.description)
                .unwrap_or_else(|| "ClawHub skill.".to_owned()),
            trust_level: "community".to_owned(),
            installable: status == "ready",
            installed: installed(installed_entry_ids, &request.entry_id),
            tags: detail.topics,
            version,
            homepage_url: Some(clawhub_homepage_url(&key.slug, owner_handle)),
        },
        validation: SkillCatalogValidationPayload {
            status: status.to_owned(),
            issues,
            issue_codes,
        },
        readme_preview: markdown.map(|value| value.chars().take(2_000).collect()),
        files: None,
    })
}

async fn materialize_github_skill(
    github_ref: GithubSkillRef,
    source_id: &str,
    source_label: &str,
    entry_id: String,
    homepage_url: Option<String>,
    progress: Option<CatalogInstallProgressSink<'_>>,
) -> Result<MaterializedCatalogSkill, CommandErrorPayload> {
    emit_catalog_progress(progress, "resolving", 10);
    let commit =
        resolve_github_commit(&github_ref.owner, &github_ref.repo, &github_ref.reference).await?;
    emit_catalog_progress(progress, "checking", 18);
    let tree = fetch_github_tree(&github_ref.owner, &github_ref.repo, &commit).await?;
    let temp_dir = tempfile::tempdir()
        .map_err(|error| runtime_operation_failed(format!("catalog temp dir failed: {error}")))?;
    let package_path = catalog_package_path(temp_dir.path())?;
    fs::create_dir_all(&package_path).map_err(|error| {
        runtime_operation_failed(format!("catalog package dir failed: {error}"))
    })?;

    let prefix = format!("{}/", github_ref.path.trim_matches('/'));
    let files = tree
        .tree
        .into_iter()
        .filter(|item| item.kind == "blob")
        .filter_map(|item| {
            let relative = item.path.strip_prefix(&prefix)?.to_owned();
            Some((item.path, relative, item.size))
        })
        .collect::<Vec<_>>();
    if !files.iter().any(|(_, relative, _)| relative == "SKILL.md") {
        return Err(invalid_payload(
            "catalog skill must contain SKILL.md".to_owned(),
        ));
    }
    emit_catalog_progress(progress, "downloading", 25);
    let total_files = files.len().max(1);
    let mut file_count = 0_usize;
    let mut total_bytes = 0_usize;
    for (source_path, relative, declared_size) in files {
        file_count += 1;
        if file_count > MAX_CATALOG_PACKAGE_FILES {
            return Err(invalid_payload(
                "skill package has too many files".to_owned(),
            ));
        }
        if declared_size
            .and_then(|size| usize::try_from(size).ok())
            .is_some_and(|size| size > MAX_CATALOG_PACKAGE_FILE_BYTES)
        {
            return Err(invalid_payload(
                "skill package file is too large".to_owned(),
            ));
        }
        let destination = safe_join(&package_path, Path::new(&relative))?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                runtime_operation_failed(format!("catalog package parent create failed: {error}"))
            })?;
        }
        let content = fetch_raw_github_file_bytes(
            &github_ref.owner,
            &github_ref.repo,
            &commit,
            &source_path,
            MAX_CATALOG_PACKAGE_FILE_BYTES,
            "skill package file is too large",
        )
        .await?;
        total_bytes = total_bytes.saturating_add(content.len());
        if total_bytes > MAX_CATALOG_PACKAGE_BYTES {
            return Err(invalid_payload("skill package is too large".to_owned()));
        }
        fs::write(&destination, content).map_err(|error| {
            runtime_operation_failed(format!("catalog package file write failed: {error}"))
        })?;
        let percent = 25 + ((file_count * 35) / total_files).min(35);
        emit_catalog_progress(progress, "downloading", percent as u8);
    }
    Ok(MaterializedCatalogSkill {
        temp_dir,
        package_path,
        origin: SkillInstallOriginRecord {
            source_id: source_id.to_owned(),
            source_label: source_label.to_owned(),
            entry_id,
            version: Some(github_ref.reference),
            commit_sha: Some(commit),
            homepage_url,
            installed_from_catalog: true,
        },
    })
}

async fn materialize_clawhub_skill(
    request: InstallSkillFromCatalogRequest,
    progress: Option<CatalogInstallProgressSink<'_>>,
) -> Result<MaterializedCatalogSkill, CommandErrorPayload> {
    let key = parse_clawhub_entry_id(&request.entry_id)?;
    let client = http_client()?;
    emit_catalog_progress(progress, "checking", 15);
    let scan_status = fetch_clawhub_scan_status(
        &client,
        &key.slug,
        key.owner_handle.as_deref(),
        request.version.as_deref(),
    )
    .await?;
    if !clawhub_scan_allows_install(scan_status.as_deref()) {
        return Err(invalid_payload(
            "ClawHub skill scan is not clean".to_owned(),
        ));
    }
    let mut url = reqwest::Url::parse_with_params(
        "https://clawhub.ai/api/v1/download",
        [("slug", key.slug.as_str())],
    )
    .map_err(|error| {
        runtime_operation_failed(format!("ClawHub download URL build failed: {error}"))
    })?;
    append_owner_handle(&mut url, key.owner_handle.as_deref());
    if let Some(version) = request.version.as_deref() {
        url.query_pairs_mut().append_pair("version", version);
    }
    let response =
        ensure_success(client.get(url).send().await.map_err(|error| {
            runtime_operation_failed(format!("ClawHub download failed: {error}"))
        })?)
        .await?;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    if content_type.contains("application/json") {
        let handoff = response
            .json::<ClawHubGithubHandoff>()
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("ClawHub handoff parse failed: {error}"))
            })?;
        if handoff.source_ref != "public-github" {
            return Err(invalid_payload(
                "unsupported ClawHub handoff source".to_owned(),
            ));
        }
        let (owner, repo) = handoff
            .repo
            .split_once('/')
            .ok_or_else(|| invalid_payload("invalid ClawHub GitHub repo handoff".to_owned()))?;
        return materialize_github_skill(
            GithubSkillRef {
                owner: owner.to_owned(),
                repo: repo.to_owned(),
                reference: handoff.commit.clone(),
                path: handoff.path,
            },
            "clawhub",
            "ClawHub",
            request.entry_id,
            Some(handoff.archive_url),
            progress,
        )
        .await;
    }
    emit_catalog_progress(progress, "downloading", 25);
    let bytes = read_response_bytes_limited(
        response,
        MAX_CATALOG_DOWNLOAD_BYTES,
        "catalog download is too large",
        "ClawHub download bytes failed",
        progress,
    )
    .await?;
    let temp_dir = tempfile::tempdir()
        .map_err(|error| runtime_operation_failed(format!("catalog temp dir failed: {error}")))?;
    let package_path = catalog_package_path(temp_dir.path())?;
    fs::create_dir_all(&package_path).map_err(|error| {
        runtime_operation_failed(format!("catalog package dir failed: {error}"))
    })?;
    unpack_zip_skill_package(&bytes, &package_path)?;
    Ok(MaterializedCatalogSkill {
        temp_dir,
        package_path,
        origin: SkillInstallOriginRecord {
            source_id: "clawhub".to_owned(),
            source_label: "ClawHub".to_owned(),
            entry_id: request.entry_id,
            version: request.version,
            commit_sha: None,
            homepage_url: Some(clawhub_homepage_url(&key.slug, key.owner_handle.as_deref())),
            installed_from_catalog: true,
        },
    })
}

fn unpack_zip_skill_package(bytes: &[u8], destination: &Path) -> Result<(), CommandErrorPayload> {
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader)
        .map_err(|error| invalid_payload(format!("catalog ZIP could not be opened: {error}")))?;
    let mut root_prefix: Option<PathBuf> = None;
    let mut file_count = 0_usize;
    let mut total_bytes = 0_usize;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| invalid_payload(format!("catalog ZIP entry invalid: {error}")))?;
        let Some(enclosed_name) = file.enclosed_name() else {
            return Err(invalid_payload(
                "catalog ZIP contains unsafe path".to_owned(),
            ));
        };
        let relative = strip_single_root(&enclosed_name, &mut root_prefix);
        if relative.as_os_str().is_empty() {
            continue;
        }
        let output_path = safe_join(destination, &relative)?;
        if file.is_dir() {
            fs::create_dir_all(&output_path).map_err(|error| {
                runtime_operation_failed(format!("catalog ZIP dir failed: {error}"))
            })?;
            continue;
        }
        file_count += 1;
        if file_count > MAX_CATALOG_PACKAGE_FILES {
            return Err(invalid_payload(
                "skill package has too many files".to_owned(),
            ));
        }
        let file_size = usize::try_from(file.size())
            .map_err(|_| invalid_payload("skill package file is too large".to_owned()))?;
        if file_size > MAX_CATALOG_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "skill package file is too large".to_owned(),
            ));
        }
        total_bytes = total_bytes.saturating_add(file_size);
        if total_bytes > MAX_CATALOG_PACKAGE_BYTES {
            return Err(invalid_payload("skill package is too large".to_owned()));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                runtime_operation_failed(format!("catalog ZIP parent failed: {error}"))
            })?;
        }
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .map_err(|error| invalid_payload(format!("catalog ZIP file read failed: {error}")))?;
        if content.len() > MAX_CATALOG_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "skill package file is too large".to_owned(),
            ));
        }
        fs::write(&output_path, content).map_err(|error| {
            runtime_operation_failed(format!("catalog ZIP file write failed: {error}"))
        })?;
    }
    if !destination.join("SKILL.md").is_file() {
        return Err(invalid_payload(
            "catalog skill must contain SKILL.md".to_owned(),
        ));
    }
    Ok(())
}

fn strip_single_root(path: &Path, root_prefix: &mut Option<PathBuf>) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Normal(first)) = components.next() else {
        return path.to_path_buf();
    };
    let first = PathBuf::from(first);
    if root_prefix.is_none() {
        *root_prefix = Some(first.clone());
    }
    if root_prefix.as_ref() == Some(&first) {
        components.as_path().to_path_buf()
    } else {
        path.to_path_buf()
    }
}

async fn fetch_github_skill_summary(
    owner: &str,
    repo: &str,
    reference: &str,
    path: &str,
) -> Result<SkillFrontmatterSummary, CommandErrorPayload> {
    let markdown =
        fetch_raw_github_file(owner, repo, reference, &format!("{path}/SKILL.md")).await?;
    parse_skill_frontmatter_summary(&markdown)
        .ok_or_else(|| invalid_payload("catalog SKILL.md frontmatter is invalid".to_owned()))
}

async fn fetch_github_tree(
    owner: &str,
    repo: &str,
    reference: &str,
) -> Result<GithubTreeResponse, CommandErrorPayload> {
    let client = http_client()?;
    let url =
        format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{reference}?recursive=1");
    ensure_success(client.get(url).send().await.map_err(|error| {
        runtime_operation_failed(format!("GitHub tree request failed: {error}"))
    })?)
    .await?
    .json::<GithubTreeResponse>()
    .await
    .map_err(|error| {
        runtime_operation_failed(format!("GitHub tree response parse failed: {error}"))
    })
}

async fn resolve_github_commit(
    owner: &str,
    repo: &str,
    reference: &str,
) -> Result<String, CommandErrorPayload> {
    if reference.len() == 40 && reference.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Ok(reference.to_owned());
    }
    let client = http_client()?;
    let url = format!("https://api.github.com/repos/{owner}/{repo}/commits/{reference}");
    let response = ensure_success(client.get(url).send().await.map_err(|error| {
        runtime_operation_failed(format!("GitHub commit request failed: {error}"))
    })?)
    .await?;
    response
        .json::<GithubCommitResponse>()
        .await
        .map(|payload| payload.sha)
        .map_err(|error| {
            runtime_operation_failed(format!("GitHub commit response parse failed: {error}"))
        })
}

async fn fetch_raw_github_file(
    owner: &str,
    repo: &str,
    reference: &str,
    path: &str,
) -> Result<String, CommandErrorPayload> {
    let bytes = fetch_raw_github_file_bytes(
        owner,
        repo,
        reference,
        path,
        MAX_CATALOG_PREVIEW_BYTES,
        "catalog text file is too large",
    )
    .await?;
    String::from_utf8(bytes)
        .map_err(|_| invalid_payload("catalog text file must be valid UTF-8".to_owned()))
}

async fn fetch_raw_github_file_preview(
    owner: &str,
    repo: &str,
    reference: &str,
    source_path: &str,
    display_path: &str,
) -> Result<GetSkillCatalogFileResponse, CommandErrorPayload> {
    let client = http_client()?;
    let url = format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{reference}/{}",
        source_path.trim_start_matches('/')
    );
    let response = ensure_success(client.get(url).send().await.map_err(|error| {
        runtime_operation_failed(format!("GitHub raw request failed: {error}"))
    })?)
    .await?;
    let (content, truncated) =
        read_response_text_preview(response, "GitHub raw bytes failed").await?;
    Ok(catalog_file_response(display_path, content, truncated))
}

async fn fetch_raw_github_file_bytes(
    owner: &str,
    repo: &str,
    reference: &str,
    path: &str,
    max_bytes: usize,
    too_large_message: &str,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let client = http_client()?;
    let url = format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{reference}/{}",
        path.trim_start_matches('/')
    );
    let response = ensure_success(client.get(url).send().await.map_err(|error| {
        runtime_operation_failed(format!("GitHub raw request failed: {error}"))
    })?)
    .await?;
    read_response_bytes_limited(
        response,
        max_bytes,
        too_large_message,
        "GitHub raw bytes failed",
        None,
    )
    .await
}

async fn fetch_clawhub_file(
    client: &reqwest::Client,
    slug: &str,
    owner_handle: Option<&str>,
    version: Option<&str>,
    path: &str,
) -> Result<String, CommandErrorPayload> {
    let mut url = reqwest::Url::parse_with_params(
        &format!("https://clawhub.ai/api/v1/skills/{slug}/file"),
        [("path", path)],
    )
    .map_err(|error| runtime_operation_failed(format!("ClawHub file URL build failed: {error}")))?;
    append_owner_handle(&mut url, owner_handle);
    if let Some(version) = version {
        url.query_pairs_mut().append_pair("version", version);
    }
    let response = ensure_success(client.get(url).send().await.map_err(|error| {
        runtime_operation_failed(format!("ClawHub file request failed: {error}"))
    })?)
    .await?;
    let bytes = read_response_bytes_limited(
        response,
        MAX_CATALOG_PREVIEW_BYTES,
        "catalog text file is too large",
        "ClawHub file read failed",
        None,
    )
    .await?;
    String::from_utf8(bytes)
        .map_err(|_| invalid_payload("catalog text file must be valid UTF-8".to_owned()))
}

async fn fetch_clawhub_file_preview(
    client: &reqwest::Client,
    slug: &str,
    owner_handle: Option<&str>,
    version: Option<&str>,
    path: &str,
) -> Result<GetSkillCatalogFileResponse, CommandErrorPayload> {
    let mut url = reqwest::Url::parse_with_params(
        &format!("https://clawhub.ai/api/v1/skills/{slug}/file"),
        [("path", path)],
    )
    .map_err(|error| runtime_operation_failed(format!("ClawHub file URL build failed: {error}")))?;
    append_owner_handle(&mut url, owner_handle);
    if let Some(version) = version {
        url.query_pairs_mut().append_pair("version", version);
    }
    let response = ensure_success(client.get(url).send().await.map_err(|error| {
        runtime_operation_failed(format!("ClawHub file request failed: {error}"))
    })?)
    .await?;
    let (content, truncated) =
        read_response_text_preview(response, "ClawHub file read failed").await?;
    Ok(catalog_file_response(path, content, truncated))
}

async fn read_response_text_preview(
    mut response: reqwest::Response,
    read_context: &str,
) -> Result<(String, bool), CommandErrorPayload> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| runtime_operation_failed(format!("{read_context}: {error}")))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_CATALOG_PREVIEW_BYTES {
            break;
        }
    }
    let truncated = bytes.len() > MAX_CATALOG_PREVIEW_BYTES;
    if truncated {
        bytes.truncate(MAX_CATALOG_PREVIEW_BYTES);
    }
    match std::str::from_utf8(&bytes) {
        Ok(_) => {}
        Err(error) if truncated && error.error_len().is_none() => {
            bytes.truncate(error.valid_up_to());
        }
        Err(_) => {
            return Err(invalid_payload(
                "catalog text file must be valid UTF-8".to_owned(),
            ));
        }
    }
    String::from_utf8(bytes)
        .map(|content| (content, truncated))
        .map_err(|_| invalid_payload("catalog text file must be valid UTF-8".to_owned()))
}

async fn read_response_bytes_limited(
    mut response: reqwest::Response,
    max_bytes: usize,
    too_large_message: &str,
    read_context: &str,
    progress: Option<CatalogInstallProgressSink<'_>>,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let content_length = response.content_length();
    if content_length.is_some_and(|length| length > max_bytes as u64) {
        return Err(invalid_payload(too_large_message.to_owned()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| runtime_operation_failed(format!("{read_context}: {error}")))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > max_bytes {
            return Err(invalid_payload(too_large_message.to_owned()));
        }
        if let Some(content_length) = content_length.filter(|length| *length > 0) {
            let ratio = (bytes.len() as f64 / content_length as f64).clamp(0.0, 1.0);
            let percent = 25 + (ratio * 35.0).round() as u8;
            emit_catalog_progress(progress, "downloading", percent.min(60));
        }
    }
    emit_catalog_progress(progress, "downloading", 60);
    Ok(bytes)
}

async fn fetch_clawhub_scan_status(
    client: &reqwest::Client,
    slug: &str,
    owner_handle: Option<&str>,
    version: Option<&str>,
) -> Result<Option<String>, CommandErrorPayload> {
    let mut url = reqwest::Url::parse(&format!("https://clawhub.ai/api/v1/skills/{slug}/scan"))
        .map_err(|error| {
            runtime_operation_failed(format!("ClawHub scan URL build failed: {error}"))
        })?;
    append_owner_handle(&mut url, owner_handle);
    if let Some(version) = version {
        url.query_pairs_mut().append_pair("version", version);
    }
    let response = ensure_success(client.get(url).send().await.map_err(|error| {
        runtime_operation_failed(format!("ClawHub scan request failed: {error}"))
    })?)
    .await?;
    let payload = response
        .json::<ClawHubScanResponse>()
        .await
        .map_err(|error| runtime_operation_failed(format!("ClawHub scan parse failed: {error}")))?;
    Ok(payload.status.or_else(|| {
        payload.security.and_then(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    }))
}

async fn ensure_success(
    response: reqwest::Response,
) -> Result<reqwest::Response, CommandErrorPayload> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.text().await.unwrap_or_default();
    let mut message = format!("catalog HTTP request failed with status {status}");
    if let Some(retry_after) = retry_after {
        message.push_str(&format!("; retry after {retry_after}s"));
    }
    if !body.trim().is_empty() {
        message.push_str(": ");
        message.push_str(body.trim());
    }
    Err(runtime_operation_failed(message))
}

fn http_client() -> Result<reqwest::Client, CommandErrorPayload> {
    reqwest::Client::builder()
        .user_agent(CATALOG_USER_AGENT)
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                ACCEPT,
                "application/json, text/plain, application/zip"
                    .parse()
                    .expect("static header"),
            );
            headers.insert(
                USER_AGENT,
                CATALOG_USER_AGENT.parse().expect("static header"),
            );
            headers
        })
        .build()
        .map_err(|error| runtime_operation_failed(format!("catalog HTTP client failed: {error}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillFrontmatterSummary {
    name: String,
    description: String,
    tags: Vec<String>,
}

fn parse_skill_frontmatter_summary(markdown: &str) -> Option<SkillFrontmatterSummary> {
    let frontmatter = markdown.strip_prefix("---\n")?.split_once("\n---")?.0;
    let name = frontmatter_string(frontmatter, "name")?;
    let description = frontmatter_string(frontmatter, "description")?;
    let tags = frontmatter_inline_array(frontmatter, "tags");
    Some(SkillFrontmatterSummary {
        name,
        description,
        tags,
    })
}

fn validate_catalog_markdown(markdown: &str) -> SkillCatalogValidationPayload {
    let mut issues = Vec::new();
    let mut issue_codes = Vec::new();
    let Some(summary) = parse_skill_frontmatter_summary(markdown) else {
        return SkillCatalogValidationPayload {
            status: "blocked".to_owned(),
            issues: vec!["SKILL.md must contain name and description frontmatter.".to_owned()],
            issue_codes: vec!["frontmatter_required".to_owned()],
        };
    };
    if summary.name.chars().count() > 64 {
        issues.push("Skill name exceeds 64 characters.".to_owned());
        issue_codes.push("skill_name_too_long".to_owned());
    }
    if !summary
        .name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        issues.push("Skill name must use lowercase letters, numbers, and hyphens.".to_owned());
        issue_codes.push("skill_name_format".to_owned());
    }
    SkillCatalogValidationPayload {
        status: if issues.is_empty() {
            "ready"
        } else {
            "blocked"
        }
        .to_owned(),
        issues,
        issue_codes,
    }
}

fn frontmatter_string(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    frontmatter.lines().find_map(|line| {
        let value = line.trim().strip_prefix(&prefix)?.trim();
        Some(value.trim_matches('"').trim_matches('\'').to_owned())
    })
}

fn frontmatter_inline_array(frontmatter: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key}:");
    frontmatter
        .lines()
        .find_map(|line| {
            let value = line.trim().strip_prefix(&prefix)?.trim();
            let value = value.strip_prefix('[')?.strip_suffix(']')?;
            Some(
                value
                    .split(',')
                    .map(str::trim)
                    .map(|item| item.trim_matches('"').trim_matches('\'').to_owned())
                    .filter(|item| !item.is_empty())
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn markdown_links(markdown: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();
    for line in markdown.lines() {
        let mut rest = line;
        while let Some(label_start) = rest.find('[') {
            let Some(label_end) = rest[label_start + 1..].find(']') else {
                break;
            };
            let label_end = label_start + 1 + label_end;
            if !rest[label_end + 1..].starts_with('(') {
                rest = &rest[label_end + 1..];
                continue;
            }
            let Some(url_end) = rest[label_end + 2..].find(')') else {
                break;
            };
            let url_end = label_end + 2 + url_end;
            links.push((
                rest[label_start + 1..label_end].to_owned(),
                rest[label_end + 2..url_end].to_owned(),
            ));
            rest = &rest[url_end + 1..];
        }
    }
    links
}

fn parse_github_tree_url(value: &str) -> Option<GithubSkillRef> {
    let url = value
        .strip_prefix("https://github.com/")
        .or_else(|| value.strip_prefix("http://github.com/"))?;
    let parts = url.split('/').collect::<Vec<_>>();
    if parts.len() < 5 {
        return None;
    }
    let owner = parts[0];
    let repo = parts[1];
    let mode = parts[2];
    if !matches!(mode, "tree" | "blob") {
        return None;
    }
    let reference = parts[3];
    let path = parts[4..].join("/");
    if owner.is_empty() || repo.is_empty() || reference.is_empty() || path.is_empty() {
        return None;
    }
    Some(GithubSkillRef {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        reference: reference.to_owned(),
        path: path.trim_end_matches("/SKILL.md").to_owned(),
    })
}

fn parse_awesome_entry_id(entry_id: &str) -> Result<GithubSkillRef, CommandErrorPayload> {
    let raw = entry_tail(entry_id, "awesome-agent-skills:")?;
    let parts = raw.split('|').collect::<Vec<_>>();
    if parts.len() != 4 {
        return Err(invalid_payload(
            "invalid Awesome Agent Skills entry id".to_owned(),
        ));
    }
    Ok(GithubSkillRef {
        owner: parts[0].to_owned(),
        repo: parts[1].to_owned(),
        reference: parts[2].to_owned(),
        path: parts[3].to_owned(),
    })
}

fn awesome_entry_id(github_ref: &GithubSkillRef) -> String {
    format!(
        "awesome-agent-skills:{}|{}|{}|{}",
        github_ref.owner, github_ref.repo, github_ref.reference, github_ref.path
    )
}

async fn ensure_awesome_entry_allowed(
    github_ref: &GithubSkillRef,
) -> Result<(), CommandErrorPayload> {
    let markdown =
        fetch_raw_github_file(AWESOME_OWNER, AWESOME_REPO, AWESOME_REPO_REF, "README.md").await?;
    if awesome_markdown_contains_entry(&markdown, github_ref) {
        Ok(())
    } else {
        Err(invalid_payload(
            "Awesome Agent Skills entry is not in catalog".to_owned(),
        ))
    }
}

fn awesome_markdown_contains_entry(markdown: &str, github_ref: &GithubSkillRef) -> bool {
    markdown_links(markdown)
        .into_iter()
        .filter_map(|(_, url)| parse_github_tree_url(&url))
        .any(|candidate| candidate == *github_ref)
}

fn anthropic_skill_dir_from_tree_path(path: &str) -> Option<String> {
    let dir = path.strip_suffix("/SKILL.md")?;
    if dir.trim().is_empty() {
        None
    } else {
        Some(dir.to_owned())
    }
}

fn paginate_catalog_entries(
    entries: Vec<SkillCatalogEntryPayload>,
    cursor: Option<&str>,
    limit: Option<u32>,
) -> ListSkillCatalogEntriesResponse {
    let Some(limit) = limit.and_then(|value| usize::try_from(value).ok()) else {
        return ListSkillCatalogEntriesResponse {
            entries,
            next_cursor: None,
        };
    };
    let limit = limit.clamp(1, 100);
    let offset = cursor
        .and_then(|value| value.strip_prefix("offset:"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let total = entries.len();
    let page_entries = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let next_offset = offset.saturating_add(page_entries.len());
    let next_cursor = (next_offset < total).then(|| format!("offset:{next_offset}"));

    ListSkillCatalogEntriesResponse {
        entries: page_entries,
        next_cursor,
    }
}

fn clawhub_entry_id(item: &ClawHubSkillItem) -> String {
    match item
        .owner_handle
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(owner_handle) => format!("clawhub:{owner_handle}/{}", item.slug),
        None => format!("clawhub:{}", item.slug),
    }
}

fn parse_clawhub_entry_id(entry_id: &str) -> Result<ClawHubEntryKey, CommandErrorPayload> {
    let raw = entry_tail(entry_id, "clawhub:")?;
    let (owner_handle, slug) = match raw.split_once('/') {
        Some((owner_handle, slug)) => {
            if owner_handle.trim().is_empty() || slug.trim().is_empty() {
                return Err(invalid_payload("invalid ClawHub entry id".to_owned()));
            }
            (Some(owner_handle.to_owned()), slug.to_owned())
        }
        None => (None, raw.to_owned()),
    };

    Ok(ClawHubEntryKey { owner_handle, slug })
}

fn append_owner_handle(url: &mut reqwest::Url, owner_handle: Option<&str>) {
    if let Some(owner_handle) = owner_handle.filter(|value| !value.trim().is_empty()) {
        url.query_pairs_mut()
            .append_pair("ownerHandle", owner_handle.trim());
    }
}

fn clawhub_homepage_url(slug: &str, owner_handle: Option<&str>) -> String {
    let mut url = reqwest::Url::parse(&format!("https://clawhub.ai/skills/{slug}"))
        .unwrap_or_else(|_| reqwest::Url::parse("https://clawhub.ai/skills").expect("static URL"));
    append_owner_handle(&mut url, owner_handle);
    url.to_string()
}

pub fn mark_catalog_entry_name_conflict(response: &mut GetSkillCatalogEntryResponse) {
    if response.entry.installed || response.validation.status == "blocked" {
        return;
    }
    response.entry.installable = false;
    response.validation.status = "blocked".to_owned();
    response.validation.issues.push(format!(
        "Active skill name already exists: {}",
        response.entry.name
    ));
    response
        .validation
        .issue_codes
        .push("active_skill_name_exists".to_owned());
}

fn catalog_file_response(
    path: &str,
    content: String,
    truncated: bool,
) -> GetSkillCatalogFileResponse {
    GetSkillCatalogFileResponse {
        file: SkillCatalogFileContentPayload {
            path: path.to_owned(),
            content,
            truncated,
        },
    }
}

fn ensure_catalog_file_path(path: &str) -> Result<String, CommandErrorPayload> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(invalid_payload("catalog file path is required".to_owned()));
    }
    let relative = Path::new(trimmed);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(invalid_payload("catalog file path is unsafe".to_owned()));
    }
    Ok(trimmed.to_owned())
}

fn catalog_file_source_path(skill_path: &str, file_path: &str) -> String {
    let skill_path = skill_path.trim_matches('/');
    if skill_path.is_empty() {
        file_path.trim_start_matches('/').to_owned()
    } else {
        format!("{}/{}", skill_path, file_path.trim_start_matches('/'))
    }
}

fn entry_tail<'a>(entry_id: &'a str, prefix: &str) -> Result<&'a str, CommandErrorPayload> {
    entry_id
        .strip_prefix(prefix)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_payload("catalog entry id does not match source".to_owned()))
}

fn safe_join(root: &Path, relative: &Path) -> Result<PathBuf, CommandErrorPayload> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(invalid_payload(
            "catalog package contains unsafe path".to_owned(),
        ));
    }
    Ok(root.join(relative))
}

fn catalog_package_path(temp_root: &Path) -> Result<PathBuf, CommandErrorPayload> {
    let canonical_root = temp_root.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("catalog temp dir canonicalize failed: {error}"))
    })?;
    Ok(canonical_root.join("package"))
}

fn emit_catalog_progress(
    progress: Option<CatalogInstallProgressSink<'_>>,
    stage: &str,
    percent: u8,
) {
    if let Some(progress) = progress {
        progress(stage, percent.min(100));
    }
}

fn invalid_payload(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "INVALID_PAYLOAD",
        message,
    }
}

fn runtime_operation_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn catalog_sources_are_fixed_and_spec_is_not_installable() {
        let sources = fixed_catalog_sources();

        assert_eq!(sources.len(), 4);
        assert!(sources
            .iter()
            .any(|source| source.id == "anthropic" && source.installable));
        assert!(sources
            .iter()
            .any(|source| source.id == "agent-skills-spec" && !source.installable));
    }

    #[test]
    fn clawhub_scan_gate_rejects_unknown_or_unsafe_status() {
        assert!(clawhub_scan_allows_install(Some("clean")));
        assert!(!clawhub_scan_allows_install(Some("suspicious")));
        assert!(!clawhub_scan_allows_install(Some("malicious")));
        assert!(!clawhub_scan_allows_install(None));
    }

    #[test]
    fn github_tree_links_parse_to_repo_ref_and_path() {
        let parsed =
            parse_github_tree_url("https://github.com/anthropics/skills/tree/main/frontend-design")
                .expect("github tree link should parse");

        assert_eq!(parsed.owner, "anthropics");
        assert_eq!(parsed.repo, "skills");
        assert_eq!(parsed.reference, "main");
        assert_eq!(parsed.path, "frontend-design");
    }

    #[test]
    fn github_blob_skill_links_parse_to_skill_directory() {
        let parsed =
            parse_github_tree_url("https://github.com/example/skills/blob/main/foo/SKILL.md")
                .expect("github blob link should parse");

        assert_eq!(parsed.owner, "example");
        assert_eq!(parsed.repo, "skills");
        assert_eq!(parsed.reference, "main");
        assert_eq!(parsed.path, "foo");
    }

    #[test]
    fn catalog_file_paths_reject_escape_and_empty_values() {
        assert!(ensure_catalog_file_path("SKILL.md").is_ok());
        assert!(ensure_catalog_file_path("references/guide.md").is_ok());
        assert!(ensure_catalog_file_path("").is_err());
        assert!(ensure_catalog_file_path("/SKILL.md").is_err());
        assert!(ensure_catalog_file_path("../SKILL.md").is_err());
        assert!(ensure_catalog_file_path("references/../SKILL.md").is_err());
    }

    #[test]
    fn catalog_file_response_tracks_truncation() {
        let response = catalog_file_response("SKILL.md", "hello".to_owned(), true);

        assert_eq!(response.file.path, "SKILL.md");
        assert_eq!(response.file.content, "hello");
        assert!(response.file.truncated);
    }

    #[test]
    fn catalog_entry_name_conflict_blocks_install() {
        let mut response = GetSkillCatalogEntryResponse {
            entry: SkillCatalogEntryPayload {
                source_id: "anthropic".to_owned(),
                source_label: "Anthropic Skills".to_owned(),
                entry_id: "anthropic:frontend-design".to_owned(),
                name: "frontend-design".to_owned(),
                description: "Design frontend interfaces.".to_owned(),
                trust_level: "official".to_owned(),
                installable: true,
                installed: false,
                tags: Vec::new(),
                version: Some("main".to_owned()),
                homepage_url: None,
            },
            validation: SkillCatalogValidationPayload {
                status: "ready".to_owned(),
                issues: Vec::new(),
                issue_codes: Vec::new(),
            },
            readme_preview: None,
            files: None,
        };

        mark_catalog_entry_name_conflict(&mut response);

        assert!(!response.entry.installable);
        assert_eq!(response.validation.status, "blocked");
        assert_eq!(
            response.validation.issue_codes,
            vec!["active_skill_name_exists".to_owned()]
        );
    }

    #[test]
    fn anthropic_skill_paths_include_nested_skills_directory() {
        assert_eq!(
            anthropic_skill_dir_from_tree_path("template/SKILL.md"),
            Some("template".to_owned())
        );
        assert_eq!(
            anthropic_skill_dir_from_tree_path("skills/frontend-design/SKILL.md"),
            Some("skills/frontend-design".to_owned())
        );
        assert_eq!(anthropic_skill_dir_from_tree_path("README.md"), None);
    }

    #[test]
    fn catalog_entries_can_be_offset_paginated() {
        let entries = (0..5)
            .map(|index| SkillCatalogEntryPayload {
                source_id: "anthropic".to_owned(),
                source_label: "Anthropic Skills".to_owned(),
                entry_id: format!("anthropic:skill-{index}"),
                name: format!("skill-{index}"),
                description: "Skill package.".to_owned(),
                trust_level: "official".to_owned(),
                installable: true,
                installed: false,
                tags: Vec::new(),
                version: Some("main".to_owned()),
                homepage_url: None,
            })
            .collect::<Vec<_>>();

        let first_page = paginate_catalog_entries(entries.clone(), None, Some(2));
        assert_eq!(first_page.entries.len(), 2);
        assert_eq!(first_page.entries[0].entry_id, "anthropic:skill-0");
        assert_eq!(first_page.next_cursor.as_deref(), Some("offset:2"));

        let second_page = paginate_catalog_entries(entries, Some("offset:2"), Some(2));
        assert_eq!(second_page.entries.len(), 2);
        assert_eq!(second_page.entries[0].entry_id, "anthropic:skill-2");
        assert_eq!(second_page.next_cursor.as_deref(), Some("offset:4"));
    }

    #[test]
    fn clawhub_search_results_parse_as_list_items() {
        let payload = serde_json::json!({
            "results": [
                {
                    "slug": "self-improving-agent",
                    "displayName": "Self Improving Agent",
                    "summary": "Log learnings after each task.",
                    "topics": ["self-improvement"],
                    "ownerHandle": "kingaiwork",
                    "version": "1.0.0"
                }
            ],
            "nextCursor": "cursor-2"
        });

        let parsed: ClawHubListResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].owner_handle.as_deref(), Some("kingaiwork"));
        assert_eq!(
            clawhub_entry_id(&parsed.items[0]),
            "clawhub:kingaiwork/self-improving-agent"
        );
    }

    #[test]
    fn clawhub_detail_response_accepts_wrapped_skill_payload() {
        let payload = serde_json::json!({
            "skill": {
                "slug": "skill-vetter",
                "displayName": "Skill Vetter",
                "summary": "Security-first skill vetting.",
                "topics": ["security"],
                "latestVersion": { "version": "1.0.0" }
            }
        });

        let parsed: ClawHubDetailResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(parsed.skill.slug, "skill-vetter");
        assert_eq!(parsed.skill.display_name.as_deref(), Some("Skill Vetter"));
    }

    #[test]
    fn awesome_entry_must_exist_in_catalog_markdown() {
        let markdown = "- [Listed](https://github.com/example/skills/tree/main/listed)\n";
        let listed = GithubSkillRef {
            owner: "example".to_owned(),
            repo: "skills".to_owned(),
            reference: "main".to_owned(),
            path: "listed".to_owned(),
        };
        let unlisted = GithubSkillRef {
            owner: "example".to_owned(),
            repo: "other".to_owned(),
            reference: "main".to_owned(),
            path: "listed".to_owned(),
        };

        assert!(awesome_markdown_contains_entry(markdown, &listed));
        assert!(!awesome_markdown_contains_entry(markdown, &unlisted));
    }

    #[test]
    fn zip_package_rejects_large_decompressed_file() {
        let mut cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default();
        writer.start_file("skill/SKILL.md", options).unwrap();
        writer
            .write_all(b"---\nname: test\ndescription: test\n---\n")
            .unwrap();
        writer.start_file("skill/blob.bin", options).unwrap();
        writer.write_all(&vec![0_u8; 1024 * 1024 + 1]).unwrap();
        writer.finish().unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let destination = temp_dir.path().join("package");
        fs::create_dir_all(&destination).unwrap();
        let error =
            unpack_zip_skill_package(cursor.get_ref(), &destination).expect_err("large file fails");

        assert_eq!(error.code, "INVALID_PAYLOAD");
        assert_eq!(error.message, "skill package file is too large");
    }

    #[cfg(unix)]
    #[test]
    fn catalog_package_path_canonicalizes_symlinked_temp_root() {
        let real_temp_root = tempfile::tempdir().unwrap();
        let link_parent = tempfile::tempdir().unwrap();
        let link_root = link_parent.path().join("catalog-temp-link");
        std::os::unix::fs::symlink(real_temp_root.path(), &link_root).unwrap();

        let package_path = catalog_package_path(&link_root).unwrap();

        assert!(package_path.starts_with(real_temp_root.path().canonicalize().unwrap()));
        assert!(!package_path.starts_with(&link_root));
        assert_eq!(package_path.file_name().unwrap(), "package");
    }
}
