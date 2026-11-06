//! Rune page import: the one write Pix makes to the League client (SPEC §2, rule 2's exception).
//!
//! - Only when you click a rune page in Draft; nothing automatic.
//! - Only `/lol-perks/` endpoints. CI fails if this file names any other client endpoint, and
//!   every other client module must stay GET-only.
//! - Pix writes one page named "Pix: …": it updates that page if it exists, else creates one, and
//!   if you're out of page slots it overwrites your current editable page (as U.GG and Blitz do).

use reqwest::Method;
use reqwest::header::AUTHORIZATION;
use serde_json::{Value, json};

use super::http::{LcuError, LcuHttp};
use crate::builds::RunePage;

pub const PAGE_PREFIX: &str = "Pix:";
const PAGES: &str = "/lol-perks/v1/pages";
const CURRENT_PAGE: &str = "/lol-perks/v1/currentpage";

async fn send(http: &LcuHttp, method: Method, path: &str, body: &Value) -> Result<(), LcuError> {
    debug_assert!(path.starts_with("/lol-perks/"), "rune import only touches /lol-perks/");
    let (client, base, auth) = http.parts();
    let resp = client.request(method, format!("{base}{path}")).header(AUTHORIZATION, auth).json(body).send().await?;
    if !resp.status().is_success() {
        return Err(LcuError::Status { status: resp.status().as_u16(), path: path.to_owned() });
    }
    Ok(())
}

/// The page body: 4 primary runes, 2 secondary runes, then 3 stat shards.
pub fn page_body(name: &str, page: &RunePage) -> Value {
    let mut perks = page.perks.clone();
    perks.extend(&page.shards);
    json!({
        "name": name,
        "primaryStyleId": page.primary_style,
        "subStyleId": page.sub_style,
        "selectedPerkIds": perks,
        "current": true
    })
}

fn flag(p: &Value, key: &str) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// An existing Pix page to update in place.
pub fn pix_page(pages: &[Value]) -> Option<i64> {
    pages
        .iter()
        .find(|p| flag(p, "isEditable") && p.get("name").and_then(Value::as_str).is_some_and(|n| n.starts_with(PAGE_PREFIX)))
        .and_then(|p| p.get("id")?.as_i64())
}

/// With no free slot: the current page if it's editable, else the first editable custom page.
pub fn page_to_replace(pages: &[Value]) -> Option<i64> {
    let editable = |p: &&Value| flag(p, "isEditable") && flag(p, "isDeletable");
    pages
        .iter()
        .filter(editable)
        .find(|p| flag(p, "current"))
        .or_else(|| pages.iter().find(editable))
        .and_then(|p| p.get("id")?.as_i64())
}

async fn overwrite(http: &LcuHttp, id: i64, body: &Value) -> Result<(), LcuError> {
    let mut body = body.clone();
    body["id"] = json!(id);
    send(http, Method::PUT, &format!("{PAGES}/{id}"), &body).await?;
    send(http, Method::PUT, CURRENT_PAGE, &json!(id)).await
}

/// Imports `page` as your current rune page, named `name` (prefixed with "Pix:").
pub async fn import(http: &LcuHttp, name: &str, page: &RunePage) -> Result<(), String> {
    let name = format!("{PAGE_PREFIX} {name}");
    let body = page_body(&name, page);
    let pages: Vec<Value> = http.get_json(PAGES).await.map_err(|e| e.to_string())?;
    if let Some(id) = pix_page(&pages) {
        return overwrite(http, id, &body).await.map_err(|e| e.to_string());
    }
    if send(http, Method::POST, PAGES, &body).await.is_ok() {
        return Ok(());
    }
    // Most likely every page slot is used: reuse the current editable page.
    match page_to_replace(&pages) {
        Some(id) => overwrite(http, id, &body).await.map_err(|e| e.to_string()),
        None => Err("No editable rune page to use. Create one in the client, then try again.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: i64, name: &str, current: bool, editable: bool) -> Value {
        json!({"id": id, "name": name, "current": current, "isEditable": editable, "isDeletable": editable})
    }

    #[test]
    fn body_has_six_runes_then_three_shards() {
        let p = RunePage { primary_style: 8000, sub_style: 8100, perks: vec![9923, 8139, 8138, 8135, 9111, 8014], shards: vec![5005, 5008, 5001] };
        let b = page_body("Pix: Briar Top", &p);
        assert_eq!(b["selectedPerkIds"].as_array().unwrap().len(), 9);
        assert_eq!(b["selectedPerkIds"][0], 9923);
        assert_eq!(b["selectedPerkIds"][8], 5001);
        assert_eq!((b["primaryStyleId"].as_u64(), b["subStyleId"].as_u64()), (Some(8000), Some(8100)));
        assert_eq!(b["current"], true);
    }

    #[test]
    fn updates_an_existing_pix_page_first() {
        let pages = vec![page(1, "Default", false, false), page(7, "My Ahri", true, true), page(9, "Pix: Ahri Mid", false, true)];
        assert_eq!(pix_page(&pages), Some(9));
        assert_eq!(pix_page(&[page(9, "Pix: locked", false, false)]), None, "only editable pages");
    }

    #[test]
    fn with_no_free_slot_replaces_the_current_editable_page() {
        let pages = vec![page(1, "Preset", true, false), page(7, "My Ahri", false, true), page(8, "My Zed", false, true)];
        assert_eq!(page_to_replace(&pages), Some(7), "current page is a locked preset: first editable one");
        let pages = vec![page(7, "My Ahri", false, true), page(8, "My Zed", true, true)];
        assert_eq!(page_to_replace(&pages), Some(8), "the current page when it's editable");
        assert_eq!(page_to_replace(&[page(1, "Preset", true, false)]), None);
    }
}
