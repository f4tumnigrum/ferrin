//! [`ToolSet`]: an insertion-ordered map of tool names to tools.

use std::sync::Arc;

use ferrin_spec::ToolName;
use indexmap::IndexMap;

use crate::error::DuplicateToolError;
use crate::tool::Tool;

/// Named tools available to a call. Insertion order is preserved; names are
/// unique.
#[derive(Debug, Clone, Default)]
pub struct ToolSet {
    tools: IndexMap<ToolName, Arc<Tool>>,
}

impl ToolSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tool, consuming and returning the set.
    ///
    /// # Errors
    ///
    /// Returns [`DuplicateToolError`] when `name` is already present.
    pub fn insert(
        mut self,
        name: impl Into<ToolName>,
        tool: Tool,
    ) -> Result<Self, DuplicateToolError> {
        self.try_insert(name, tool)?;
        Ok(self)
    }

    /// Adds a tool in place.
    ///
    /// # Errors
    ///
    /// Returns [`DuplicateToolError`] when `name` is already present.
    pub fn try_insert(
        &mut self,
        name: impl Into<ToolName>,
        tool: Tool,
    ) -> Result<(), DuplicateToolError> {
        self.try_insert_arc(name, Arc::new(tool))
    }

    /// Adds a shared tool in place.
    ///
    /// # Errors
    ///
    /// Returns [`DuplicateToolError`] when `name` is already present.
    pub fn try_insert_arc(
        &mut self,
        name: impl Into<ToolName>,
        tool: Arc<Tool>,
    ) -> Result<(), DuplicateToolError> {
        let name = name.into();
        if self.tools.contains_key(&name) {
            return Err(DuplicateToolError { name });
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// Inserts or replaces a tool, keeping its position when replacing.
    pub fn replace(&mut self, name: impl Into<ToolName>, tool: Arc<Tool>) -> Option<Arc<Tool>> {
        self.tools.insert(name.into(), tool)
    }

    /// Removes a tool, preserving the order of the others.
    pub fn remove(&mut self, name: &str) -> Option<Arc<Tool>> {
        self.tools.shift_remove(name)
    }

    /// Looks up a tool.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<Tool>> {
        self.tools.get(name)
    }

    /// Returns `true` when `name` is present.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Tool names in insertion order.
    pub fn names(&self) -> impl Iterator<Item = &ToolName> + '_ {
        self.tools.keys()
    }

    /// Tools in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&ToolName, &Arc<Tool>)> + '_ {
        self.tools.iter()
    }

    /// Number of tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Returns `true` when the set has no tools.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Keeps only the named tools (order of this set). Unknown names are
    /// ignored.
    #[must_use]
    pub fn filter_active(&self, active: &[ToolName]) -> Self {
        Self {
            tools: self
                .tools
                .iter()
                .filter(|(name, _)| active.contains(name))
                .map(|(name, tool)| (name.clone(), Arc::clone(tool)))
                .collect(),
        }
    }

    /// Merges another set into this one.
    ///
    /// # Errors
    ///
    /// Returns [`DuplicateToolError`] for the first name present in both.
    pub fn merge(mut self, other: Self) -> Result<Self, DuplicateToolError> {
        for (name, tool) in other.tools {
            self.try_insert_arc(name, tool)?;
        }
        Ok(self)
    }

    /// Tools in sending order: names listed in `order` first (in that order),
    /// the rest sorted alphabetically.
    #[must_use]
    pub fn ordered(&self, order: &[ToolName]) -> Vec<(&ToolName, &Arc<Tool>)> {
        let mut listed: Vec<(&ToolName, &Arc<Tool>)> = order
            .iter()
            .filter_map(|name| self.tools.get_key_value(name))
            .collect();
        let mut rest: Vec<(&ToolName, &Arc<Tool>)> = self
            .tools
            .iter()
            .filter(|(name, _)| !order.contains(name))
            .collect();
        rest.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
        listed.append(&mut rest);
        listed
    }
}

impl<'a> IntoIterator for &'a ToolSet {
    type Item = (&'a ToolName, &'a Arc<Tool>);
    type IntoIter = indexmap::map::Iter<'a, ToolName, Arc<Tool>>;

    fn into_iter(self) -> Self::IntoIter {
        self.tools.iter()
    }
}
