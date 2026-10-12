//! End-to-end tests of `2bec tags` and `2bec badges` against a mock API. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const TAG: &str = "11111111-1111-4111-8111-111111111111";
const ROBOT: &str = "22222222-2222-4222-8222-222222222222";
const BADGE: &str = "33333333-3333-4333-8333-333333333333";

fn tag(id: &str, name: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": name, "comment": "Release runs", "kind": kind})
}

fn badge(enabled: bool) -> serde_json::Value {
    serde_json::json!({
        "id": BADGE, "name": "Prod", "description": "Production releases", "enabled": enabled,
        "priority": 20, "color": 3, "symbolKind": "icon", "symbol": "rocket", "tagMatch": "all",
        "statusFilter": "any", "validFrom": "2026-01-01T00:00:00+00:00", "validUntil": null,
        "tags": [tag(TAG, "release", "business")],
        "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": "2026-10-01T10:00:00+02:00"
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/tags", "traceId": "trace123"
        }))
}

fn cmd(server: &MockServer, dir: &TempDir) -> Command {
    let mut c = Command::cargo_bin("2bec").unwrap();
    c.isolate_env()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .env("TWOBEC_TOKEN", KEY);
    c
}

// ---------- tags ----------

#[tokio::test(flavor = "multi_thread")]
async fn tags_list_filters_locally_by_kind_and_words() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [tag(ROBOT, "obs::browser", "robot"), tag(TAG, "release 42", "business")],
            "next": null, "total": 2
        })))
        .expect(2)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["tags", "list"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ID\tNAME\tKIND\tCOMMENT\n"))
        .stdout(predicate::str::contains(format!(
            "{ROBOT}\tobs::browser\trobot\tRelease runs\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{TAG}\trelease 42\tbusiness\tRelease runs\n"
        )));
    cmd(&server, &dir)
        .args(["tags", "list", "--kind", "business", "-q", "RELEASE 42"])
        .assert()
        .success()
        .stdout(predicate::str::contains("release 42"))
        .stdout(predicate::str::contains("obs::browser").not());
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_get_prints_the_etag_and_checks_the_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/tags/{TAG}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v2\"")
                .set_body_json(tag(TAG, "release 42", "business")),
        )
        .expect(2)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["tags", "get", TAG])
        .assert()
        .success()
        .stdout(predicate::str::contains("Name:     release 42\n"))
        .stdout(predicate::str::contains("Kind:     business\n"))
        .stdout(predicate::str::contains("ETag:     \"v2\"\n"));
    cmd(&server, &dir)
        .args(["tags", "get", TAG, "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"etag\":\"\\\"v2\\\"\""));
    cmd(&server, &dir)
        .args(["tags", "get", "release"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("tag id must be a GUID"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_create_sends_name_and_comment_and_rejects_reserved_names() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/tags"))
        .and(body_json(
            serde_json::json!({"name": "release 42", "comment": "Release runs"}),
        ))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"v1\"")
                .set_body_json(tag(TAG, "release 42", "business")),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/tags"))
        .and(body_json(serde_json::json!({"name": "dup"})))
        .respond_with(problem("conflict", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "tags",
            "create",
            " release 42 ",
            "--comment",
            "Release runs",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Created tag release 42 ({TAG})"
        )))
        .stdout(predicate::str::contains("ETag:     \"v1\"\n"));
    cmd(&server, &dir)
        .args(["tags", "create", "obs::mine"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("reserved for the platform"));
    cmd(&server, &dir)
        .args(["tags", "create", "dup"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("conflict"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_edit_sends_a_merge_patch_with_if_match() {
    let server = MockServer::start().await;
    let mut renamed = tag(TAG, "release 43", "business");
    renamed["comment"] = serde_json::Value::Null;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/tags/{TAG}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "\"v2\""))
        .and(body_json(
            serde_json::json!({"name": "release 43", "comment": null}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v3\"")
                .set_body_json(renamed),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "tags",
            "edit",
            TAG,
            "--name",
            "release 43",
            "--comment",
            "",
            "--if-match",
            "\"v2\"",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Updated tag release 43"))
        .stdout(predicate::str::contains("Comment:  -\n"))
        .stdout(predicate::str::contains("ETag:     \"v3\"\n"));
    cmd(&server, &dir)
        .args(["tags", "edit", TAG])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_delete_sends_force_and_reports_tag_in_use() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/tags/{TAG}")))
        .and(query_param("force", "true"))
        .and(header("if-match", "\"v2\""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/tags/{ROBOT}")))
        .and(query_param_is_missing("force"))
        .respond_with(problem("tag-in-use", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["tags", "rm", TAG, "--force", "--if-match", "\"v2\""])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Deleted tag {TAG}")));
    cmd(&server, &dir)
        .args(["tags", "delete", ROBOT])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("tag-in-use"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_scope_missing_names_the_scope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/tags"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["tags", "create", "x"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `indicators:write`",
        ));
}

// ---------- badges ----------

#[tokio::test(flavor = "multi_thread")]
async fn badges_list_prints_the_rule_and_filters_by_state() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/badges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [badge(true)], "next": null, "total": 1
        })))
        .expect(2)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["badges", "list"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tNAME\tSTATE\tPRIORITY\tSYMBOL\tCOLOR\tRULE\tVALIDITY\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{BADGE}\tProd\tenabled\t20\ticon:rocket\t3\tevery run with all of: release\tfrom 2026-01-01 00:00\n"
        )));
    cmd(&server, &dir)
        .args(["badges", "list", "--disabled"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Prod").not());
}

#[tokio::test(flavor = "multi_thread")]
async fn badges_get_prints_the_tags_and_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/badges/{BADGE}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v2\"")
                .set_body_json(badge(true)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["badges", "get", BADGE])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Description:  Production releases\n",
        ))
        .stdout(predicate::str::contains(
            "Symbol:       icon:rocket (color 3)\n",
        ))
        .stdout(predicate::str::contains(
            "Rule:         every run with all of: release\n",
        ))
        .stdout(predicate::str::contains("Updated:      2026-10-01 10:00\n"))
        .stdout(predicate::str::contains("ETag:         \"v2\"\n"))
        .stdout(predicate::str::contains(format!(
            "{TAG}\trelease\tbusiness\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn badges_create_sends_the_definition_and_checks_it_locally() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/badges"))
        .and(body_json(serde_json::json!({
            "name": "Prod", "description": "Production releases", "enabled": false,
            "priority": 20, "color": 3, "symbolKind": "icon", "symbol": "rocket",
            "tagMatch": "all", "statusFilter": "any", "validFrom": "2026-01-01T00:00:00Z",
            "tags": [TAG]
        })))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"v1\"")
                .set_body_json(badge(false)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "badges",
            "create",
            "--name",
            "Prod",
            "--description",
            "Production releases",
            "--disabled",
            "--priority",
            "20",
            "--color",
            "3",
            "--symbol",
            "rocket",
            "--icon",
            "--tag",
            TAG,
            "--tag",
            &TAG.to_uppercase(),
            "--all-tags",
            "--status",
            "any",
            "--valid-from",
            "2026-01-01T00:00:00Z",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Created badge Prod ({BADGE})"
        )))
        .stdout(predicate::str::contains("State:        disabled\n"))
        .stdout(predicate::str::contains("ETag:         \"v1\"\n"));
    cmd(&server, &dir)
        .args(["badges", "create", "--name", "Prod", "--symbol", "P"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("at least one --tag is required"));
    cmd(&server, &dir)
        .args([
            "badges", "create", "--name", "Prod", "--symbol", "PRO", "--tag", TAG,
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("2 characters at most"));
    cmd(&server, &dir)
        .args([
            "badges", "create", "--name", "Prod", "--symbol", "P", "--tag", TAG, "--color", "9",
        ])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn badges_edit_sends_a_merge_patch_and_clears_members() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/badges/{BADGE}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "\"v2\""))
        .and(body_json(serde_json::json!({
            "enabled": false, "symbolKind": "text", "symbol": "P", "validFrom": null
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v3\"")
                .set_body_json(badge(false)),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/badges/{BADGE}")))
        .and(body_json(serde_json::json!({"name": "dup"})))
        .respond_with(problem("conflict", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "badges",
            "edit",
            BADGE,
            "--enabled",
            "false",
            "--symbol",
            "P",
            "--clear-valid-from",
            "--if-match",
            "\"v2\"",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Updated badge Prod"))
        .stdout(predicate::str::contains("ETag:         \"v3\"\n"));
    cmd(&server, &dir)
        .args(["badges", "edit", BADGE])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
    cmd(&server, &dir)
        .args(["badges", "edit", BADGE, "--name", "dup"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("conflict"));
}

#[tokio::test(flavor = "multi_thread")]
async fn badges_delete_sends_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/badges/{BADGE}")))
        .and(header("if-match", "\"v2\""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["badges", "rm", BADGE, "--if-match", "\"v2\""])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Deleted badge {BADGE}")));
}
