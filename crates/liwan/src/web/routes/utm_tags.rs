use aide::axum::{ApiRouter, IntoApiResponse, routing::*};
use axum::{
    Json,
    extract::{Path, State},
};
use http::StatusCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::app::{
    UtmTagInput,
    models::UtmTag,
    reports::{self, DateRange, UtmTagSummary},
};
use crate::web::RouterState;
use crate::web::routes::dashboard::{report_project, run_report};
use crate::web::session::{Admin, MaybeAuth};
use crate::web::webext::{ApiResult, AxumErrExt, empty_response, http_bail};

pub fn router() -> ApiRouter<RouterState> {
    ApiRouter::new()
        .api_route("/project/{project_id}/utm-tags", get(list_handler))
        .api_route("/project/{project_id}/utm-tags", post(create_handler))
        // The static segment wins over `{tag_id}`, so the summary is never read as a tag id.
        .api_route("/project/{project_id}/utm-tags/summary", post(summary_handler))
        .api_route("/project/{project_id}/utm-tags/{tag_id}", put(update_handler))
        .api_route("/project/{project_id}/utm-tags/{tag_id}", delete(delete_handler))
}

#[derive(Serialize, JsonSchema, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct UtmTagResponse {
    id: String,
    display_name: String,
    url: String,
    utm_source: Option<String>,
    utm_medium: Option<String>,
    utm_campaign: Option<String>,
    utm_content: Option<String>,
    utm_term: Option<String>,
}

#[derive(Serialize, JsonSchema, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct UtmTagsResponse {
    utm_tags: Vec<UtmTagResponse>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct UtmTagRequest {
    display_name: String,
    url: String,
    #[serde(default)]
    utm_source: Option<String>,
    #[serde(default)]
    utm_medium: Option<String>,
    #[serde(default)]
    utm_campaign: Option<String>,
    #[serde(default)]
    utm_content: Option<String>,
    #[serde(default)]
    utm_term: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
struct UtmTagSummaryRequest {
    range: DateRange,
}

#[derive(Serialize, JsonSchema, Debug, Clone)]
struct UtmTagSummaryResponse {
    summaries: Vec<UtmTagSummary>,
}

impl From<&UtmTag> for UtmTagResponse {
    fn from(tag: &UtmTag) -> Self {
        Self {
            id: tag.id.clone(),
            display_name: tag.display_name.clone(),
            url: tag.url.clone(),
            utm_source: tag.utm_source.clone(),
            utm_medium: tag.utm_medium.clone(),
            utm_campaign: tag.utm_campaign.clone(),
            utm_content: tag.utm_content.clone(),
            utm_term: tag.utm_term.clone(),
        }
    }
}

impl From<UtmTagRequest> for UtmTagInput {
    fn from(request: UtmTagRequest) -> Self {
        Self {
            display_name: request.display_name,
            url: request.url,
            utm_source: request.utm_source,
            utm_medium: request.utm_medium,
            utm_campaign: request.utm_campaign,
            utm_content: request.utm_content,
            utm_term: request.utm_term,
        }
    }
}

/// Return 404 unless the project exists; the mutation handlers are admin-only,
/// so a visible project is the only extra check they need.
async fn require_project(app: &RouterState, project_id: &str) -> ApiResult<()> {
    let app = app.app.clone();
    let project_id = project_id.to_string();
    spawn_blocking(move || app.projects.get(&project_id))
        .await
        .http_status(StatusCode::INTERNAL_SERVER_ERROR)?
        .http_status(StatusCode::NOT_FOUND)?;
    Ok(())
}

async fn list_handler(
    app: State<RouterState>,
    Path(project_id): Path<String>,
    MaybeAuth(user): MaybeAuth,
) -> ApiResult<Json<UtmTagsResponse>> {
    report_project(&app, project_id.clone(), user).await?;
    let app = app.app.clone();
    let tags = spawn_blocking(move || app.utm_tags.list(&project_id))
        .await
        .http_status(StatusCode::INTERNAL_SERVER_ERROR)?
        .http_err("Failed to list UTM tags", StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(UtmTagsResponse { utm_tags: tags.iter().map(UtmTagResponse::from).collect() }))
}

fn saved(tag: UtmTag) -> Json<UtmTagResponse> {
    Json(UtmTagResponse::from(&tag))
}

async fn create_handler(
    app: State<RouterState>,
    Path(project_id): Path<String>,
    Admin(_): Admin,
    Json(request): Json<UtmTagRequest>,
) -> ApiResult<Json<UtmTagResponse>> {
    require_project(&app, &project_id).await?;
    let app = app.app.clone();
    let input: UtmTagInput = request.into();
    let tag = spawn_blocking(move || app.utm_tags.create(&project_id, &input))
        .await
        .http_status(StatusCode::INTERNAL_SERVER_ERROR)?
        .http_err("Failed to create UTM tag", StatusCode::BAD_REQUEST)?;
    Ok(saved(tag))
}

async fn update_handler(
    app: State<RouterState>,
    Path((project_id, tag_id)): Path<(String, String)>,
    Admin(_): Admin,
    Json(request): Json<UtmTagRequest>,
) -> ApiResult<Json<UtmTagResponse>> {
    require_project(&app, &project_id).await?;
    let app = app.app.clone();
    let input: UtmTagInput = request.into();
    let tag = spawn_blocking(move || app.utm_tags.update(&project_id, &tag_id, &input))
        .await
        .http_status(StatusCode::INTERNAL_SERVER_ERROR)?
        .http_err("Failed to update UTM tag", StatusCode::BAD_REQUEST)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(saved(tag))
}

async fn delete_handler(
    app: State<RouterState>,
    Path((project_id, tag_id)): Path<(String, String)>,
    Admin(_): Admin,
) -> ApiResult<impl IntoApiResponse> {
    require_project(&app, &project_id).await?;
    let app = app.app.clone();
    let deleted = spawn_blocking(move || app.utm_tags.delete(&project_id, &tag_id))
        .await
        .http_status(StatusCode::INTERNAL_SERVER_ERROR)?
        .http_err("Failed to delete UTM tag", StatusCode::INTERNAL_SERVER_ERROR)?;
    if !deleted {
        http_bail!(StatusCode::NOT_FOUND, "UTM tag not found");
    }
    Ok(empty_response())
}

async fn summary_handler(
    app: State<RouterState>,
    Path(project_id): Path<String>,
    MaybeAuth(user): MaybeAuth,
    Json(request): Json<UtmTagSummaryRequest>,
) -> ApiResult<Json<UtmTagSummaryResponse>> {
    let (entities, _) = report_project(&app, project_id.clone(), user).await?;
    reports::validate_request(&request.range, &[], &app.config.limits).http_status(StatusCode::BAD_REQUEST)?;
    let lookup = app.app.clone();
    let tags = spawn_blocking(move || lookup.utm_tags.list(&project_id))
        .await
        .http_status(StatusCode::INTERNAL_SERVER_ERROR)?
        .http_err("Failed to list UTM tags", StatusCode::INTERNAL_SERVER_ERROR)?;
    let summaries =
        run_report(&app, move |conn| reports::utm_tag_summary(conn, &entities, &tags, &request.range)).await?;
    Ok(Json(UtmTagSummaryResponse { summaries }))
}
