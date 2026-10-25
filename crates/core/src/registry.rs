use std::sync::Arc;

use indexmap::IndexMap;
use infermark_sdk::{BenchModule, Scorer};

/// Every module, scorer and exporter available in this build.
///
/// Built-ins are added by [`Registry::builtin`]; extra modules (behind Cargo features, or from
/// downstream crates) are added with the `register_*` methods.
#[derive(Default, Clone)]
pub struct Registry {
    modules: IndexMap<String, Arc<dyn BenchModule>>,
    scorers: IndexMap<String, Arc<dyn Scorer>>,
}

impl Registry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// The modules and scorers that ship with InferMark.
    pub fn builtin() -> Self {
        let mut r = Self::empty();
        crate::modules::register_builtin(&mut r);
        crate::scorers::register_builtin(&mut r);
        r
    }

    /// Add a module. A module with the same id replaces the existing one, which lets
    /// downstream builds override built-ins.
    pub fn register_module(&mut self, module: impl BenchModule + 'static) -> &mut Self {
        let id = module.descriptor().id;
        self.modules.insert(id, Arc::new(module));
        self
    }

    pub fn register_scorer(&mut self, scorer: impl Scorer + 'static) -> &mut Self {
        self.scorers
            .insert(scorer.id().to_string(), Arc::new(scorer));
        self
    }

    pub fn module(&self, id: &str) -> Option<&Arc<dyn BenchModule>> {
        self.modules.get(id)
    }

    pub fn modules(&self) -> impl Iterator<Item = &Arc<dyn BenchModule>> {
        self.modules.values()
    }

    pub fn scorer(&self, id: &str) -> Option<&Arc<dyn Scorer>> {
        self.scorers.get(id)
    }

    pub fn scorer_ids(&self) -> impl Iterator<Item = &str> {
        self.scorers.keys().map(String::as_str)
    }
}
