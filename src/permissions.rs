use std::collections::HashMap;

/// The kind of CRUD action a tool performs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Read,
    Create,
    Update,
    Delete,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Read => write!(f, "read"),
            Action::Create => write!(f, "create"),
            Action::Update => write!(f, "update"),
            Action::Delete => write!(f, "delete"),
        }
    }
}

/// Controls which CRUD actions are allowed, globally and per-resource.
///
/// Read is always allowed and cannot be disabled.
#[derive(Clone, Debug)]
pub struct ActionPermissions {
    global_create_enabled: bool,
    global_update_enabled: bool,
    global_delete_enabled: bool,
    /// Per-resource overrides. Key is `"<resource>-<action>"` (e.g. `"deployment-delete"`),
    /// value is `false` to disable.
    resource_overrides: HashMap<String, bool>,
    /// When `false`, the `decode: true` parameter in `get_secret` is ignored
    /// and secret values are never returned.
    pub secret_decode_enabled: bool,
    /// When `false`, the `apply_manifest` tool is blocked regardless of other
    /// permission settings.
    pub apply_manifest_enabled: bool,
}

impl ActionPermissions {
    /// Build permissions from CLI / env flags.
    ///
    /// * `disable_create` / `disable_update` / `disable_delete` — global kill-switches.
    /// * `disable_actions` — per-resource entries like `"deployment-delete"`, `"pod-create"`.
    pub fn new(
        disable_create: bool,
        disable_update: bool,
        disable_delete: bool,
        disable_actions: Vec<String>,
        secret_decode_enabled: bool,
        apply_manifest_enabled: bool,
    ) -> Self {
        let mut resource_overrides = HashMap::new();
        for entry in &disable_actions {
            let entry = entry.trim();
            if !entry.is_empty() {
                // Store the full key (e.g. "deployment-delete") → disabled
                resource_overrides.insert(entry.to_lowercase(), false);
            }
        }

        Self {
            global_create_enabled: !disable_create,
            global_update_enabled: !disable_update,
            global_delete_enabled: !disable_delete,
            resource_overrides,
            secret_decode_enabled,
            apply_manifest_enabled,
        }
    }

    /// Check whether `action` is allowed for `resource`.
    ///
    /// Read is always allowed.  For other actions the per-resource override
    /// takes precedence over the global flag.
    pub fn is_action_allowed(&self, resource: &str, action: &Action) -> bool {
        match action {
            Action::Read => true,
            Action::Create | Action::Update | Action::Delete => {
                let action_str = action.to_string();
                let key = format!("{}-{}", resource.to_lowercase(), action_str);

                // Per-resource override wins if present.
                if let Some(&allowed) = self.resource_overrides.get(&key) {
                    return allowed;
                }

                // Fall back to the global flag.
                match action {
                    Action::Create => self.global_create_enabled,
                    Action::Update => self.global_update_enabled,
                    Action::Delete => self.global_delete_enabled,
                    Action::Read => unreachable!(),
                }
            }
        }
    }

    /// Map a tool name (e.g. `"create_deployment"`) to the CRUD action it
    /// performs.
    pub fn action_for_tool(tool_name: &str) -> Action {
        // Exact-match read tools
        match tool_name {
            "can_i" | "whoami" | "list_my_permissions" | "get_resource_yaml" => {
                return Action::Read;
            }
            "apply_manifest" => return Action::Create,
            _ => {}
        }

        // Suffix-based read tools
        if tool_name.ends_with("_logs") || tool_name.ends_with("_metrics") {
            return Action::Read;
        }

        // Prefix-based classification
        if tool_name.starts_with("list_") || tool_name.starts_with("get_") {
            return Action::Read;
        }
        if tool_name.starts_with("create_") {
            return Action::Create;
        }
        if tool_name.starts_with("update_")
            || tool_name.starts_with("scale_")
            || tool_name.starts_with("restart_")
            || tool_name.starts_with("rollback_")
            || tool_name.starts_with("approve_")
            || tool_name.starts_with("deny_")
            || tool_name.starts_with("cordon_")
            || tool_name.starts_with("uncordon_")
            || tool_name.starts_with("drain_")
        {
            return Action::Update;
        }
        if tool_name.starts_with("delete_") || tool_name.starts_with("evict_") {
            return Action::Delete;
        }

        // Unknown tools default to Read (safe fallback — they will still hit
        // "unknown tool" downstream).
        Action::Read
    }

    /// Convenience: is a given tool allowed under current permissions?
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if tool_name == "apply_manifest" && !self.apply_manifest_enabled {
            return false;
        }
        let action = Self::action_for_tool(tool_name);
        // Extract resource from tool name by stripping the action prefix.
        let resource = Self::resource_from_tool(tool_name);
        self.is_action_allowed(&resource, &action)
    }

    /// Best-effort resource extraction from a tool name.
    ///
    /// `"create_deployment"` → `"deployment"`, `"scale_deployment"` → `"deployment"`,
    /// `"apply_manifest"` → `"manifest"`, `"get_resource_yaml"` → `"resource_yaml"`.
    fn resource_from_tool(tool_name: &str) -> String {
        let prefixes = [
            "list_",
            "get_",
            "create_",
            "update_",
            "delete_",
            "scale_",
            "restart_",
            "rollback_",
            "approve_",
            "deny_",
            "cordon_",
            "uncordon_",
            "drain_",
            "evict_",
        ];
        for prefix in &prefixes {
            if let Some(rest) = tool_name.strip_prefix(prefix) {
                return rest.to_string();
            }
        }
        tool_name.to_string()
    }
}

impl Default for ActionPermissions {
    /// All actions enabled, no overrides, secret decode enabled, apply_manifest enabled.
    fn default() -> Self {
        Self {
            global_create_enabled: true,
            global_update_enabled: true,
            global_delete_enabled: true,
            resource_overrides: HashMap::new(),
            secret_decode_enabled: true,
            apply_manifest_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Default permissions
    // -----------------------------------------------------------------------

    #[test]
    fn default_allows_everything() {
        let perms = ActionPermissions::default();
        assert!(perms.is_action_allowed("deployment", &Action::Read));
        assert!(perms.is_action_allowed("deployment", &Action::Create));
        assert!(perms.is_action_allowed("deployment", &Action::Update));
        assert!(perms.is_action_allowed("deployment", &Action::Delete));
    }

    // -----------------------------------------------------------------------
    // Read is always allowed
    // -----------------------------------------------------------------------

    #[test]
    fn read_always_allowed_even_when_all_globals_disabled() {
        let perms = ActionPermissions::new(true, true, true, vec![], true, true);
        assert!(perms.is_action_allowed("deployment", &Action::Read));
        assert!(perms.is_action_allowed("pod", &Action::Read));
    }

    // -----------------------------------------------------------------------
    // Global disable flags
    // -----------------------------------------------------------------------

    #[test]
    fn global_disable_create() {
        let perms = ActionPermissions::new(true, false, false, vec![], true, true);
        assert!(!perms.is_action_allowed("deployment", &Action::Create));
        assert!(!perms.is_action_allowed("pod", &Action::Create));
        assert!(perms.is_action_allowed("deployment", &Action::Update));
        assert!(perms.is_action_allowed("deployment", &Action::Delete));
    }

    #[test]
    fn global_disable_update() {
        let perms = ActionPermissions::new(false, true, false, vec![], true, true);
        assert!(!perms.is_action_allowed("deployment", &Action::Update));
        assert!(perms.is_action_allowed("deployment", &Action::Create));
        assert!(perms.is_action_allowed("deployment", &Action::Delete));
    }

    #[test]
    fn global_disable_delete() {
        let perms = ActionPermissions::new(false, false, true, vec![], true, true);
        assert!(!perms.is_action_allowed("pod", &Action::Delete));
        assert!(perms.is_action_allowed("pod", &Action::Create));
        assert!(perms.is_action_allowed("pod", &Action::Update));
    }

    #[test]
    fn global_disable_all_mutating() {
        let perms = ActionPermissions::new(true, true, true, vec![], true, true);
        assert!(!perms.is_action_allowed("service", &Action::Create));
        assert!(!perms.is_action_allowed("service", &Action::Update));
        assert!(!perms.is_action_allowed("service", &Action::Delete));
        assert!(perms.is_action_allowed("service", &Action::Read));
    }

    // -----------------------------------------------------------------------
    // Per-resource overrides
    // -----------------------------------------------------------------------

    #[test]
    fn per_resource_disable() {
        let perms = ActionPermissions::new(
            false,
            false,
            false,
            vec!["deployment-delete".to_string(), "pod-create".to_string()],
            true,
            true,
        );
        // Specific overrides block the action
        assert!(!perms.is_action_allowed("deployment", &Action::Delete));
        assert!(!perms.is_action_allowed("pod", &Action::Create));
        // Others still allowed globally
        assert!(perms.is_action_allowed("deployment", &Action::Create));
        assert!(perms.is_action_allowed("deployment", &Action::Update));
        assert!(perms.is_action_allowed("pod", &Action::Delete));
    }

    #[test]
    fn per_resource_override_is_case_insensitive() {
        let perms = ActionPermissions::new(
            false,
            false,
            false,
            vec!["Deployment-Delete".to_string()],
            true,
            true,
        );
        assert!(!perms.is_action_allowed("deployment", &Action::Delete));
        assert!(!perms.is_action_allowed("DEPLOYMENT", &Action::Delete));
    }

    #[test]
    fn empty_disable_actions_entries_ignored() {
        let perms = ActionPermissions::new(
            false,
            false,
            false,
            vec!["".to_string(), "  ".to_string()],
            true,
            true,
        );
        assert!(perms.is_action_allowed("deployment", &Action::Create));
        assert!(perms.is_action_allowed("deployment", &Action::Update));
        assert!(perms.is_action_allowed("deployment", &Action::Delete));
    }

    // -----------------------------------------------------------------------
    // action_for_tool mapping
    // -----------------------------------------------------------------------

    #[test]
    fn action_for_read_tools() {
        assert_eq!(
            ActionPermissions::action_for_tool("list_deployments"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("get_deployment"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("get_pod_logs"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("get_build_logs"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("get_metrics"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("list_namespaces"),
            Action::Read
        );
        assert_eq!(ActionPermissions::action_for_tool("can_i"), Action::Read);
        assert_eq!(ActionPermissions::action_for_tool("whoami"), Action::Read);
        assert_eq!(
            ActionPermissions::action_for_tool("list_my_permissions"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("get_resource_yaml"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("list_ingresses"),
            Action::Read
        );
        assert_eq!(
            ActionPermissions::action_for_tool("get_events"),
            Action::Read
        );
    }

    #[test]
    fn action_for_create_tools() {
        assert_eq!(
            ActionPermissions::action_for_tool("create_deployment"),
            Action::Create
        );
        assert_eq!(
            ActionPermissions::action_for_tool("create_service"),
            Action::Create
        );
        assert_eq!(
            ActionPermissions::action_for_tool("create_ingress"),
            Action::Create
        );
        assert_eq!(
            ActionPermissions::action_for_tool("create_pod"),
            Action::Create
        );
        assert_eq!(
            ActionPermissions::action_for_tool("apply_manifest"),
            Action::Create
        );
    }

    #[test]
    fn action_for_update_tools() {
        assert_eq!(
            ActionPermissions::action_for_tool("update_ingress"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("update_deployment"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("scale_deployment"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("restart_deployment"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("rollback_deployment"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("approve_csr"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("deny_csr"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("cordon_node"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("uncordon_node"),
            Action::Update
        );
        assert_eq!(
            ActionPermissions::action_for_tool("drain_node"),
            Action::Update
        );
    }

    #[test]
    fn action_for_delete_tools() {
        assert_eq!(
            ActionPermissions::action_for_tool("delete_deployment"),
            Action::Delete
        );
        assert_eq!(
            ActionPermissions::action_for_tool("delete_pod"),
            Action::Delete
        );
        assert_eq!(
            ActionPermissions::action_for_tool("delete_ingress"),
            Action::Delete
        );
        assert_eq!(
            ActionPermissions::action_for_tool("evict_pod"),
            Action::Delete
        );
    }

    #[test]
    fn unknown_tool_defaults_to_read() {
        assert_eq!(
            ActionPermissions::action_for_tool("some_unknown_thing"),
            Action::Read
        );
    }

    // -----------------------------------------------------------------------
    // is_tool_allowed integration
    // -----------------------------------------------------------------------

    #[test]
    fn tool_allowed_with_defaults() {
        let perms = ActionPermissions::default();
        assert!(perms.is_tool_allowed("create_deployment"));
        assert!(perms.is_tool_allowed("delete_pod"));
        assert!(perms.is_tool_allowed("list_pods"));
    }

    #[test]
    fn tool_disallowed_when_global_create_off() {
        let perms = ActionPermissions::new(true, false, false, vec![], true, true);
        assert!(!perms.is_tool_allowed("create_deployment"));
        assert!(!perms.is_tool_allowed("create_pod"));
        assert!(!perms.is_tool_allowed("apply_manifest"));
        // Read & update & delete still ok
        assert!(perms.is_tool_allowed("list_deployments"));
        assert!(perms.is_tool_allowed("update_ingress"));
        assert!(perms.is_tool_allowed("delete_pod"));
    }

    #[test]
    fn tool_disallowed_when_global_delete_off() {
        let perms = ActionPermissions::new(false, false, true, vec![], true, true);
        assert!(!perms.is_tool_allowed("delete_deployment"));
        assert!(!perms.is_tool_allowed("delete_pod"));
        assert!(!perms.is_tool_allowed("evict_pod"));
        assert!(perms.is_tool_allowed("create_deployment"));
    }

    #[test]
    fn tool_disallowed_by_per_resource_override() {
        let perms = ActionPermissions::new(
            false,
            false,
            false,
            vec!["deployment-delete".to_string()],
            true,
            true,
        );
        assert!(!perms.is_tool_allowed("delete_deployment"));
        // Other delete tools still allowed
        assert!(perms.is_tool_allowed("delete_pod"));
        // Other actions on deployment still allowed
        assert!(perms.is_tool_allowed("create_deployment"));
        assert!(perms.is_tool_allowed("update_deployment"));
    }

    #[test]
    fn read_tools_always_allowed_regardless_of_config() {
        let perms = ActionPermissions::new(true, true, true, vec![], true, true);
        assert!(perms.is_tool_allowed("list_deployments"));
        assert!(perms.is_tool_allowed("get_pod_logs"));
        assert!(perms.is_tool_allowed("can_i"));
        assert!(perms.is_tool_allowed("whoami"));
        assert!(perms.is_tool_allowed("get_resource_yaml"));
        assert!(perms.is_tool_allowed("get_metrics"));
        assert!(perms.is_tool_allowed("get_events"));
        assert!(perms.is_tool_allowed("get_build_logs"));
    }

    // -----------------------------------------------------------------------
    // resource_from_tool
    // -----------------------------------------------------------------------

    #[test]
    fn resource_extraction() {
        assert_eq!(
            ActionPermissions::resource_from_tool("create_deployment"),
            "deployment"
        );
        assert_eq!(ActionPermissions::resource_from_tool("delete_pod"), "pod");
        assert_eq!(
            ActionPermissions::resource_from_tool("scale_deployment"),
            "deployment"
        );
        assert_eq!(ActionPermissions::resource_from_tool("list_pods"), "pods");
        assert_eq!(
            ActionPermissions::resource_from_tool("apply_manifest"),
            "apply_manifest"
        );
        assert_eq!(ActionPermissions::resource_from_tool("whoami"), "whoami");
    }

    // -----------------------------------------------------------------------
    // Display for Action
    // -----------------------------------------------------------------------

    #[test]
    fn action_display() {
        assert_eq!(Action::Read.to_string(), "read");
        assert_eq!(Action::Create.to_string(), "create");
        assert_eq!(Action::Update.to_string(), "update");
        assert_eq!(Action::Delete.to_string(), "delete");
    }

    // -----------------------------------------------------------------------
    // apply_manifest_enabled flag
    // -----------------------------------------------------------------------

    #[test]
    fn apply_manifest_blocked_when_disabled() {
        let perms = ActionPermissions::new(false, false, false, vec![], true, false);
        assert!(!perms.is_tool_allowed("apply_manifest"));
        // Other create tools remain allowed
        assert!(perms.is_tool_allowed("create_deployment"));
        assert!(perms.is_tool_allowed("create_service"));
    }

    #[test]
    fn apply_manifest_allowed_when_enabled() {
        let perms = ActionPermissions::new(false, false, false, vec![], true, true);
        assert!(perms.is_tool_allowed("apply_manifest"));
    }

    #[test]
    fn apply_manifest_blocked_by_flag_even_if_create_allowed() {
        // Create is globally allowed, but apply_manifest is specifically disabled
        let perms = ActionPermissions::new(false, false, false, vec![], true, false);
        assert!(!perms.is_tool_allowed("apply_manifest"));
        assert!(perms.is_tool_allowed("create_deployment"));
    }

    #[test]
    fn apply_manifest_blocked_by_global_create_even_if_flag_enabled() {
        // apply_manifest flag is enabled but global create is disabled
        let perms = ActionPermissions::new(true, false, false, vec![], true, true);
        assert!(!perms.is_tool_allowed("apply_manifest"));
    }

    #[test]
    fn default_allows_apply_manifest() {
        let perms = ActionPermissions::default();
        assert!(perms.is_tool_allowed("apply_manifest"));
    }
}
