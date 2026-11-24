use crate::permissions::{Action, ActionPermissions};

// ---------------------------------------------------------------------------
// handle_tool metrics tests
// ---------------------------------------------------------------------------
//
// These tests verify behaviour and metric emission for the three distinct exit
// paths in handle_tool():
//   1. Permission denied  → Some(Err), emits status="denied"
//   2. Unknown tool       → None,      no metric emitted
//   3. Ok / error         → tested via the integration tests in tests/integration.rs
//
// A fresh PrometheusRecorder is installed as a thread-local override via
// metrics::with_local_recorder so tests don't interfere with each other or
// with any global recorder. block_in_place + Handle::block_on drives the
// async future from inside the sync closure.

#[cfg(test)]
mod handle_tool_metrics {
    use http::{Request, Response};
    use kube::client::Body;
    use kube::Client;
    use metrics_exporter_prometheus::PrometheusBuilder;
    use tokio::task;
    use tower_test::mock;

    use crate::permissions::ActionPermissions;
    use crate::K8sClient;

    fn mock_client_with_permissions(
        perms: ActionPermissions,
    ) -> (K8sClient, mock::Handle<Request<Body>, Response<Body>>) {
        let (svc, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(svc, "default");
        (K8sClient::new(client, vec![], perms), handle)
    }

    /// Drive an async future inside a metrics::with_local_recorder closure.
    /// Requires a multi-thread runtime so block_in_place is available.
    macro_rules! with_recorder {
        ($recorder:expr, $fut:expr) => {
            task::block_in_place(|| {
                metrics::with_local_recorder(&$recorder, || {
                    tokio::runtime::Handle::current().block_on($fut)
                })
            })
        };
    }

    // -- behaviour tests (no recorder needed) ---------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn unknown_tool_returns_none() {
        let (client, _h) = mock_client_with_permissions(ActionPermissions::default());
        let result =
            crate::mcp::handle_tool(&client, "not_a_real_tool", &serde_json::json!({})).await;
        assert!(result.is_none(), "unknown tool should return None");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn denied_tool_returns_error() {
        // disable_create=true, so create_deployment is denied
        let perms = ActionPermissions::new(true, false, false, vec![], true, true);
        let (client, _h) = mock_client_with_permissions(perms);
        let result =
            crate::mcp::handle_tool(&client, "create_deployment", &serde_json::json!({})).await;
        assert!(
            matches!(result, Some(Err(_))),
            "denied tool should return Some(Err)"
        );
    }

    // -- metric emission tests ------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn denied_tool_emits_denied_status_metric() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let prom = recorder.handle();

        let perms = ActionPermissions::new(true, false, false, vec![], true, true);
        let (client, _h) = mock_client_with_permissions(perms);

        with_recorder!(
            recorder,
            crate::mcp::handle_tool(&client, "create_deployment", &serde_json::json!({}))
        );

        let out = prom.render();
        assert!(
            out.contains("mcp_k8s_tool_calls_total"),
            "expected mcp_k8s_tool_calls_total counter\n{out}"
        );
        assert!(
            out.contains("denied"),
            "expected status=\"denied\" label\n{out}"
        );
        assert!(
            out.contains("create_deployment"),
            "expected tool=\"create_deployment\" label\n{out}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn unknown_tool_emits_no_metric() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let prom = recorder.handle();

        let (client, _h) = mock_client_with_permissions(ActionPermissions::default());

        with_recorder!(
            recorder,
            crate::mcp::handle_tool(&client, "not_a_real_tool", &serde_json::json!({}))
        );

        let out = prom.render();
        assert!(
            !out.contains("mcp_k8s_tool_calls_total"),
            "unrecognised tool should not emit a metric\n{out}"
        );
    }
}

#[test]
fn tool_definitions_returns_all_tools() {
    let perms = ActionPermissions::default();
    let tools = super::definitions::tool_definitions(&perms);
    // Verify we have a large number of tools (should be 150+)
    assert!(
        tools.len() > 100,
        "expected 100+ tools, got {}",
        tools.len()
    );
    // Verify all tools have required fields
    for tool in &tools {
        assert!(tool["name"].is_string(), "tool missing name");
        assert!(tool["description"].is_string(), "tool missing description");
        assert!(tool["inputSchema"].is_object(), "tool missing inputSchema");
    }
}

#[test]
fn tool_names_are_unique() {
    let perms = ActionPermissions::default();
    let tools = super::definitions::tool_definitions(&perms);
    let mut names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), total, "duplicate tool names found");
}

#[test]
fn disabled_create_filters_create_tools() {
    let perms = ActionPermissions::new(true, false, false, vec![], true, true);
    let tools = super::definitions::tool_definitions(&perms);
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            !name.starts_with("create_"),
            "create tool {} should be filtered",
            name
        );
        assert_ne!(name, "apply_manifest", "apply_manifest should be filtered");
    }
}

#[test]
fn disabled_delete_filters_delete_tools() {
    let perms = ActionPermissions::new(false, false, true, vec![], true, true);
    let tools = super::definitions::tool_definitions(&perms);
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            !name.starts_with("delete_"),
            "delete tool {} should be filtered",
            name
        );
    }
}

#[test]
fn disabled_update_filters_update_tools() {
    let perms = ActionPermissions::new(false, true, false, vec![], true, true);
    let tools = super::definitions::tool_definitions(&perms);
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            !name.starts_with("update_"),
            "update tool {} should be filtered",
            name
        );
        assert!(
            !name.starts_with("scale_"),
            "scale tool {} should be filtered",
            name
        );
        assert!(
            !name.starts_with("restart_"),
            "restart tool {} should be filtered",
            name
        );
        assert!(
            !name.starts_with("rollback_"),
            "rollback tool {} should be filtered",
            name
        );
    }
}

#[test]
fn read_only_mode_only_has_read_tools() {
    let perms = ActionPermissions::new(true, true, true, vec![], true, true);
    let tools = super::definitions::tool_definitions(&perms);
    // Should still have read tools
    assert!(!tools.is_empty(), "read-only mode should still have tools");
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        let action = ActionPermissions::action_for_tool(name);
        assert_eq!(
            action,
            Action::Read,
            "tool {} should be read action, got {}",
            name,
            action
        );
    }
}

#[test]
fn per_resource_disable_filters_specific_tool() {
    let perms = ActionPermissions::new(
        false,
        false,
        false,
        vec!["deployment-delete".to_string()],
        true,
        true,
    );
    let tools = super::definitions::tool_definitions(&perms);
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        !names.contains(&"delete_deployment"),
        "delete_deployment should be filtered"
    );
    // Other delete tools should still be present
    assert!(
        names.contains(&"delete_pod"),
        "delete_pod should still be present"
    );
}

#[test]
fn all_tool_schemas_are_valid() {
    let perms = ActionPermissions::default();
    let tools = super::definitions::tool_definitions(&perms);
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        let schema = &tool["inputSchema"];
        assert_eq!(
            schema["type"], "object",
            "tool {} schema type should be object",
            name
        );
        assert!(
            schema.get("properties").is_some(),
            "tool {} missing properties",
            name
        );
        assert_eq!(
            schema["additionalProperties"], false,
            "tool {} should have additionalProperties: false",
            name
        );
    }
}

#[test]
fn tool_count_matches_the_documentation() {
    // Keep in sync with README.md, CLAUDE.md and docs/src/tools/overview.md.
    let tools = super::definitions::tool_definitions(&ActionPermissions::default());
    assert_eq!(tools.len(), 211, "update the documented tool count");
}

#[test]
fn every_typed_write_tool_is_preferred_over_apply_manifest() {
    let tools = super::definitions::tool_definitions(&ActionPermissions::default());
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        if name.starts_with("create_") || name.starts_with("update_") {
            let desc = tool["description"].as_str().unwrap();
            assert!(desc.contains("apply_manifest"), "{name}: {desc}");
        }
    }
}

#[test]
fn read_only_mode_excludes_exec_pod() {
    let perms = ActionPermissions::new(true, true, true, vec![], true, true);
    let tools = super::definitions::tool_definitions(&perms);
    assert!(tools.iter().all(|t| t["name"] != "exec_pod"));
    let tools = super::definitions::tool_definitions(&ActionPermissions::default());
    assert!(tools.iter().any(|t| t["name"] == "exec_pod"));
}

#[test]
fn typed_workload_tools_are_listed() {
    let tools = super::definitions::tool_definitions(&ActionPermissions::default());
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for name in [
        "create_pod",
        "create_job",
        "create_job_from_cronjob",
        "create_cronjob",
        "update_cronjob",
        "create_deployment",
        "update_deployment",
        "create_statefulset",
        "update_statefulset",
        "create_daemonset",
        "update_daemonset",
        "create_service",
        "update_service",
        "create_ingress",
        "update_ingress",
    ] {
        assert_eq!(names.iter().filter(|n| **n == name).count(), 1, "{name}");
    }
}

#[test]
fn server_instructions_steer_to_typed_tools() {
    let text = super::SERVER_INSTRUCTIONS;
    assert!(text.contains("create_deployment") && text.contains("apply_manifest"));
    assert!(text.contains("same guardrails"));
}
