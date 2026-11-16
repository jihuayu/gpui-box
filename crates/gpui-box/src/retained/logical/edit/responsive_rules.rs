//! Scope-owned responsive declarations use the keyed edit publication boundary.
use super::*;
use crate::retained::responsive::{
    LayoutBinding, LayoutBreakpointStyles, LayoutQuery, LayoutResponsiveBindings,
};

impl LogicalEdit<'_, '_, '_> {
    fn rules(&self) -> &BTreeMap<(ScopeId, u64), LayoutBinding> {
        self.responsive_rules
            .as_ref()
            .unwrap_or(self.live_responsive_rules)
    }

    fn rules_mut(&mut self) -> &mut BTreeMap<(ScopeId, u64), LayoutBinding> {
        self.responsive_rules
            .get_or_insert_with(|| self.live_responsive_rules.clone())
    }

    /// Declare or update a rule owned by `(scope, key)`. Its target must be
    /// render output of that scope. Equal declarations retain writer identity.
    /// Changing source/target requires explicit removal before redeclaration.
    /// Automatic sessions select before builders/final publication. Otherwise
    /// call `responsive_registered` explicitly to evaluate breakpoints.
    pub fn responsive_rule(
        &mut self,
        scope: ScopeId,
        key: u64,
        source: Signal<LayoutStyle>,
        target: NodeId,
        query: LayoutQuery,
        styles: LayoutBreakpointStyles,
    ) -> Result<(), Error> {
        self.scope(scope)?;
        let dead = self.retired_scopes()?;
        if dead.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        if self
            .inserted
            .get(&target)
            .or_else(|| self.owners.get(&target))
            != Some(&scope)
        {
            return Err(Error::ScopeInUse);
        }
        self.read(source)?.validate()?;
        let old = self.rules().get(&(scope, key));
        if let Some(old) = old {
            self.render
                .validate_responsive_writer(old.source, old.target, old.binding)?;
            if old.source != source || old.target != target {
                return Err(Error::BindingConflict);
            }
            if old.query == query && old.styles == styles {
                return Ok(());
            }
        }
        if self.rules().iter().any(|(id, b)| {
            !dead.contains(&id.0)
                && *id != (scope, key)
                && (b.source == source || b.target == target)
        }) {
            return Err(Error::BindingConflict);
        }
        let binding = old.map(|b| b.binding);
        let mut rules = LayoutResponsiveBindings::new();
        rules.bindings = self
            .rules()
            .iter()
            .filter(|(id, _)| !dead.contains(&id.0) && **id != (scope, key))
            .map(|(_, b)| b.clone())
            .collect();
        rules.validate_edit(
            self.render,
            self.capabilities,
            &self.retired_outputs()?,
            Some((source, target, query, &styles)),
        )?;
        let binding = match binding {
            Some(binding) => binding,
            None => self.render.bind(source, Property::layout_style(target))?,
        };
        self.rules_mut().insert(
            (scope, key),
            LayoutBinding {
                binding,
                source,
                target,
                query,
                styles,
            },
        );
        Ok(())
    }

    /// Detach this rule's style writer. Source and last style remain; a later
    /// declaration/binding can claim the endpoint in the same edit. Removing an
    /// already observed rule cannot erase that call's final validation.
    pub fn remove_responsive_rule(&mut self, scope: ScopeId, key: u64) -> Result<(), Error> {
        self.scope(scope)?;
        let rule = self
            .rules()
            .get(&(scope, key))
            .ok_or(Error::InvalidBinding)?;
        self.render.unbind(rule.binding)?;
        self.rules_mut().remove(&(scope, key));
        Ok(())
    }

    /// Evaluate all surviving scope-owned rules at an explicit checkpoint.
    /// Does not replace an active captured environment. Prerequisite structure
    /// remains explicit; no native subscription or builder replay.
    pub fn responsive_registered(
        &mut self,
        environment: &responsive::LayoutEnvironment,
    ) -> Result<responsive::ResponsiveEditStats, Error> {
        if self.frame_environment.is_none() {
            return Err(Error::UnsafeContainerQuery);
        }
        environment.validate()?;
        self.retire_responsive_rules()?;
        let rules = LayoutResponsiveBindings {
            bindings: self.rules().values().cloned().collect(),
        };
        self.responsive(&rules, environment)
    }

    pub(super) fn settle_automatic_rules(&mut self) -> Result<(), Error> {
        if !self.rules().is_empty()
            && let Some(environment) = self.automatic_environment.clone()
        {
            self.responsive_registered(&environment)?;
        }
        Ok(())
    }

    pub(super) fn retire_responsive_rules(&mut self) -> Result<(), Error> {
        if self.rules().is_empty() {
            return Ok(());
        }
        let dead = self.retired_scopes()?;
        let retired = self
            .rules()
            .keys()
            .filter(|(owner, _)| dead.contains(owner))
            .copied()
            .collect::<Vec<_>>();
        for id in &retired {
            let rule = &self.rules()[id];
            self.render
                .validate_responsive_writer(rule.source, rule.target, rule.binding)?;
        }
        for (scope, key) in retired {
            self.remove_responsive_rule(scope, key)?;
        }
        Ok(())
    }

    pub(super) fn validate_responsive_rules(&self) -> Result<(), Error> {
        if self.rules().is_empty() {
            return Ok(());
        }
        let rules = LayoutResponsiveBindings {
            bindings: self.rules().values().cloned().collect(),
        };
        rules.validate_edit(self.render, self.capabilities, &[], None)
    }
}

#[cfg(test)]
mod tests;
