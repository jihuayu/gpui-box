//! Surface identities are monotonically increasing within one process.

#[derive(Default, Debug)]
pub(crate) struct SurfaceSession {
    latest: u64,
    attached: bool,
}

impl SurfaceSession {
    pub fn attach(&mut self, generation: u64) -> bool {
        if generation == 0 || generation <= self.latest {
            return false;
        }
        self.latest = generation;
        self.attached = true;
        true
    }

    pub fn accepts(&self, generation: u64) -> bool {
        self.attached && generation == self.latest
    }

    pub fn detach(&mut self, generation: u64) -> bool {
        if !self.accepts(generation) {
            return false;
        }
        self.attached = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_callbacks_cannot_detach_or_revive_a_surface() {
        let mut session = SurfaceSession::default();
        assert!(!session.attach(0));
        assert!(session.attach(1));
        assert!(session.detach(1));
        assert!(!session.accepts(1));
        assert!(!session.attach(1));
        assert!(session.attach(2));
        assert!(!session.detach(1));
        assert!(session.accepts(2));
    }
}
