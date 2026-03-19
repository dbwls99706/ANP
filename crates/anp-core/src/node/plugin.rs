use crate::node::registry::NodeRegistry;

/// Trait for node plugins that register custom node types.
///
/// Plugins implement this trait and call `registry.register()` for each
/// node type they provide. This is the extension point for adding custom
/// nodes without modifying the core crate.
///
/// # Example
/// ```ignore
/// struct MyPlugin;
///
/// impl NodePlugin for MyPlugin {
///     fn name(&self) -> &str { "my-plugin" }
///     fn version(&self) -> &str { "0.1.0" }
///
///     fn register(&self, registry: &mut NodeRegistry) {
///         registry.register(NodeInfo {
///             factory: || Box::new(MyCustomNode::new()),
///             schema: NodeSchema { /* ... */ },
///         });
///     }
/// }
/// ```
pub trait NodePlugin: Send + Sync {
    /// Plugin identifier (e.g. "anp-builtins", "my-custom-nodes")
    fn name(&self) -> &str;

    /// Plugin version (semver)
    fn version(&self) -> &str;

    /// Register this plugin's node types into the registry
    fn register(&self, registry: &mut NodeRegistry);
}

impl NodeRegistry {
    /// Register all nodes from a plugin
    pub fn register_plugin(&mut self, plugin: &dyn NodePlugin) {
        plugin.register(self);
    }

    /// Register multiple plugins at once
    pub fn register_plugins(&mut self, plugins: &[&dyn NodePlugin]) {
        for plugin in plugins {
            plugin.register(self);
        }
    }
}
