use crate::permissions::{Action, ActionPermissions};

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
    let perms = ActionPermissions::new(true, false, false, vec![], true);
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
    let perms = ActionPermissions::new(false, false, true, vec![], true);
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
    let perms = ActionPermissions::new(false, true, false, vec![], true);
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
    let perms = ActionPermissions::new(true, true, true, vec![], true);
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
