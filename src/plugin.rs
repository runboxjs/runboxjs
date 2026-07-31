use crate::error::Result;
use crate::vfs::Vfs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Lifecycle hooks for RunBox Extensibility Plugins.
pub trait Plugin: Send + Sync {
    /// Unique name of the plugin (e.g. "@runbox/eslint")
    fn name(&self) -> &str;

    /// Version of the plugin
    fn version(&self) -> &str;

    /// Hook executed right after a file change is registered in the VFS.
    fn on_file_change(&self, vfs: &mut Vfs, path: &str) -> Result<()> {
        let _ = (vfs, path);
        Ok(())
    }

    /// Hook executed right before a build/execution process starts.
    fn before_build(&self, vfs: &Vfs) -> Result<()> {
        let _ = vfs;
        Ok(())
    }

    /// Hook executed immediately after a build success.
    fn after_build(&self, vfs: &Vfs) -> Result<()> {
        let _ = vfs;
        Ok(())
    }
}

// ── 5.1 Plugin Manager ───────────────────────────────────────────────────────

pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    pub fn trigger_on_file_change(&self, vfs: &mut Vfs, path: &str) -> Result<()> {
        for plugin in &self.plugins {
            plugin.on_file_change(vfs, path)?;
        }
        Ok(())
    }

    pub fn trigger_before_build(&self, vfs: &Vfs) -> Result<()> {
        for plugin in &self.plugins {
            plugin.before_build(vfs)?;
        }
        Ok(())
    }

    pub fn trigger_after_build(&self, vfs: &Vfs) -> Result<()> {
        for plugin in &self.plugins {
            plugin.after_build(vfs)?;
        }
        Ok(())
    }
}

// ── Built-in Plugins (Phase 5.1 implementations) ─────────────────────────────

pub struct EslintPlugin;
impl Plugin for EslintPlugin {
    fn name(&self) -> &str {
        "eslint"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn on_file_change(&self, _vfs: &mut Vfs, path: &str) -> Result<()> {
        if path.ends_with(".js") || path.ends_with(".ts") || path.ends_with(".tsx") {
            // Simulated ESLint real-time check logic
        }
        Ok(())
    }
}

pub struct PrettierPlugin;
impl Plugin for PrettierPlugin {
    fn name(&self) -> &str {
        "prettier"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn on_file_change(&self, vfs: &mut Vfs, path: &str) -> Result<()> {
        // Pretend auto-formatting at save
        if let Ok(content) = vfs.read_string(path)
            && (path.ends_with(".js") || path.ends_with(".json"))
        {
            // In a real WASM runtime, we pass `content` to prettier format()
            let _formatted = content;
        }
        Ok(())
    }
}

pub struct TypeScriptPlugin;
impl Plugin for TypeScriptPlugin {
    fn name(&self) -> &str {
        "typescript"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn before_build(&self, _vfs: &Vfs) -> Result<()> {
        // Here we would run type-checking diagnostics and collect TSDiagnostics limits.
        Ok(())
    }
}

pub struct TailwindCssPlugin;
impl Plugin for TailwindCssPlugin {
    fn name(&self) -> &str {
        "tailwindcss"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn on_file_change(&self, _vfs: &mut Vfs, path: &str) -> Result<()> {
        if path.ends_with(".html") || path.ends_with(".tsx") || path.ends_with(".jsx") {
            // Re-run tailwindcss JIT compilation based on the token
        }
        Ok(())
    }
}

// ── Plugin Marketplace ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePlugin {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub downloads: u32,
    pub verified: bool,
}

pub struct PluginMarketplace {
    pub registry: HashMap<String, MarketplacePlugin>,
}

impl Default for PluginMarketplace {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginMarketplace {
    pub fn new() -> Self {
        let mut marketplace = Self {
            registry: HashMap::new(),
        };
        // Seed default marketplace plugins
        marketplace.seed();
        marketplace
    }

    fn seed(&mut self) {
        let default_plugins = vec![
            MarketplacePlugin {
                id: "runbox.prettier".into(),
                name: "Prettier".into(),
                description: "Opinionated code formatter for JavaScript/TypeScript".into(),
                author: "RunBox".into(),
                version: "1.0.0".into(),
                downloads: 15420,
                verified: true,
            },
            MarketplacePlugin {
                id: "runbox.eslint".into(),
                name: "ESLint".into(),
                description: "Find and fix problems in your JavaScript code".into(),
                author: "RunBox".into(),
                version: "1.0.0".into(),
                downloads: 12050,
                verified: true,
            },
        ];

        for p in default_plugins {
            self.registry.insert(p.id.clone(), p);
        }
    }

    pub fn search(&self, keyword: &str) -> Vec<&MarketplacePlugin> {
        let kw = keyword.to_lowercase();
        self.registry
            .values()
            .filter(|p| {
                p.name.to_lowercase().contains(&kw) || p.description.to_lowercase().contains(&kw)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_manager_lifecycle() {
        let mut manager = PluginManager::new();
        manager.register(Box::new(PrettierPlugin));
        manager.register(Box::new(EslintPlugin));

        let mut vfs = Vfs::new();
        vfs.write("/src/index.js", b"const a=1;".to_vec()).unwrap();

        let res = manager.trigger_on_file_change(&mut vfs, "/src/index.js");
        assert!(res.is_ok());
    }

    #[test]
    fn marketplace_search() {
        let market = PluginMarketplace::new();
        let results = market.search("formatter");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Prettier");
    }
}
