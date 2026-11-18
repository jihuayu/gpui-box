use super::*;
use std::rc::Rc;
use virtual_list::measurement::*;

impl LogicalEdit<'_, '_, '_> {
    pub(in crate::retained) fn register_list_measurement(
        &mut self,
        scope: ScopeId,
        measure: Rc<dyn Measure>,
    ) -> Result<(), Error> {
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        Rc::make_mut(
            self.list_measurements
                .get_or_insert_with(|| self.live_list_measurements.clone()),
        )
        .insert(scope, measure);
        Ok(())
    }
    pub(super) fn validate_list_measurements(
        &mut self,
        dead: &BTreeSet<ScopeId>,
    ) -> Result<(), Error> {
        if self
            .list_measurements
            .as_ref()
            .unwrap_or(self.live_list_measurements)
            .keys()
            .any(|scope| dead.contains(scope))
        {
            Rc::make_mut(
                self.list_measurements
                    .get_or_insert_with(|| self.live_list_measurements.clone()),
            )
            .retain(|scope, _| !dead.contains(scope));
        }
        let mut writers = BTreeSet::new();
        for measure in self
            .list_measurements
            .as_ref()
            .unwrap_or(self.live_list_measurements)
            .values()
        {
            for source in measure.validate(self)? {
                if !writers.insert(source) {
                    return Err(Error::InvalidVirtualList);
                }
            }
        }
        Ok(())
    }
}
impl LogicalUi {
    /// One bounded feedback step over current complete mounted lists. The host
    /// calls after source/layout settlement and before image preparation. If
    /// changed_rows>0, schedule another ordinary turn; never loop to convergence.
    /// All measured extents and anchor offsets adopt in one source transaction.
    pub fn settle_virtual_list_measurements(
        &mut self,
        max_rows: usize,
    ) -> Result<VirtualListMeasureReport, Error> {
        if max_rows == 0 {
            return Err(Error::InvalidVirtualList);
        }
        let revision = (self.ui.tree().revision, self.ui.stats().changed_sources);
        if self.measured_list_revision == Some(revision) {
            return Ok(VirtualListMeasureReport::default());
        }
        let mut pending = vec![];
        let mut report = VirtualListMeasureReport::default();
        for measure in self.list_measurements.values() {
            let update = measure.collect(self, max_rows - report.measured_rows)?;
            report.measured_rows += update.measured;
            report.changed_rows += update.changed;
            pending.push(update);
        }
        if report.changed_rows > 0 {
            let commit = self.transaction(move |tx| {
                for update in pending {
                    (update.apply)(tx)?;
                }
                Ok(())
            })?;
            report.commit = Some(commit);
        } else {
            self.measured_list_revision = Some(revision);
        }
        Ok(report)
    }
}
