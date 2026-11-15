mod common;

use anyhow::Result;
use liwan::{app::Liwan, config::Config};

#[tokio::test]
async fn pages_render_once_with_a_stable_etag() -> Result<()> {
    let client = common::TestClient::new(common::app(), common::events().0);

    let first = client.get("/").await;
    first.assert_status_success();
    let etag = first.header("etag").to_str()?.to_string();
    let body = first.text();
    assert!(body.contains("baseUrl"), "the config placeholder was not replaced");
    assert!(!body.contains("__LIWAN_CONFIG__"));

    let second = client.get("/").await;
    assert_eq!(second.header("etag").to_str()?, etag);
    assert_eq!(second.text(), body);

    let revalidated = client.get_with_headers("/", vec![("if-none-match".to_string(), etag)]).await;
    revalidated.assert_status(http::StatusCode::NOT_MODIFIED);

    Ok(())
}

#[tokio::test]
async fn rendered_pages_follow_a_base_path() -> Result<()> {
    let config = Config::load(None, [("LIWAN_BASE_URL", "https://example.com/liwan")])?;
    let app = Liwan::new_memory(config)?;
    let client = common::TestClient::new(app, common::events().0);

    let page = client.get("/liwan/").await;
    page.assert_status_success();
    assert!(page.text().contains("/liwan/_assets/"), "the base path was not applied to the embedded assets");

    Ok(())
}
