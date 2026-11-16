//! Captured host input and automatic registered-rule settlement.
use super::*;
use responsive::LayoutEnvironment;

impl LogicalUi {
    /// Last successfully published host environment. It is independent of the
    /// layout constraints and paint viewport; device/text/zoom fields are host
    /// metadata, not automatic text shaping or device projection policies.
    pub fn layout_environment(&self) -> Option<&LayoutEnvironment> {
        self.layout_session.as_ref().map(|s| &s.environment)
    }

    /// Capture host input, enable automatic scope-owned responsive rules and
    /// publish a resolved frame atomically. Later transactions/inbox drains
    /// settle those rules and publish validated local output using this input.
    pub fn frame_with_environment(
        &mut self,
        environment: &LayoutEnvironment,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
    ) -> Result<FramePacket, Error> {
        self.edit_frame(
            constraints,
            viewport,
            |_| Ok(()),
            |(), edit| edit.layout_environment(environment),
        )
        .map(|(_, frame)| frame)
    }

    /// Local-placement counterpart to `frame_with_environment`.
    pub fn local_frame_with_environment(
        &mut self,
        environment: &LayoutEnvironment,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
    ) -> Result<local::LocalFrame, Error> {
        self.edit_local_frame(
            constraints,
            viewport,
            |_| Ok(()),
            |(), edit| edit.layout_environment(environment),
        )
        .map(|(_, frame)| frame)
    }
}

#[cfg(test)]
mod tests;
