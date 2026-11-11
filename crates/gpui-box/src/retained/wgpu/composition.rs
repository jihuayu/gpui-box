use super::*;
use gpui_wgpu::{QuadDraw, QuadLayer, QuadLayerId};

fn key(id: NodeId) -> QuadLayerId {
    [id.tree, id.slot as u64, id.generation]
}

fn push_instance(draws: &mut Vec<QuadDraw>, slot: u32) {
    if let Some(QuadDraw::Instances(range)) = draws.last_mut()
        && range.end == slot
    {
        range.end += 1;
    } else {
        draws.push(QuadDraw::Instances(slot..slot + 1));
    }
}

pub(super) fn plan(
    frame: &FramePacket,
    instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
) -> Result<(Vec<QuadDraw>, Vec<QuadLayer>), Error> {
    let mut root = vec![];
    let mut stack: Vec<(NodeId, Vec<QuadDraw>)> = vec![];
    let mut layers = vec![];
    for item in frame.items.iter() {
        let mut path = vec![];
        let mut cursor = item.properties.effect;
        while let Some(id) = cursor {
            path.push(id);
            cursor = frame
                .properties
                .effects
                .get(&id)
                .ok_or(Error::InvalidFrame)?
                .parent;
            if path.len() > frame.properties.effects.len() {
                return Err(Error::InvalidFrame);
            }
        }
        path.reverse();
        let common = path
            .iter()
            .zip(&stack)
            .take_while(|(id, (open, _))| id == &open)
            .count();
        while stack.len() > common {
            let (id, draws) = stack.pop().unwrap();
            layers.push(QuadLayer {
                id: key(id),
                opacity: frame.properties.effects[&id].value,
                draws,
            });
        }
        for id in &path[common..] {
            let draws = stack.last_mut().map_or(&mut root, |(_, draws)| draws);
            draws.push(QuadDraw::Layer(key(*id)));
            stack.push((*id, vec![]));
        }
        let draws = stack.last_mut().map_or(&mut root, |(_, draws)| draws);
        for handle in instances.get(&item.node).ok_or(Error::InvalidFrame)? {
            push_instance(draws, handle.slot);
        }
        if item.image.is_some() {
            draws.push(QuadDraw::Image(key(item.node)));
        }
    }
    while let Some((id, draws)) = stack.pop() {
        layers.push(QuadLayer {
            id: key(id),
            opacity: frame.properties.effects[&id].value,
            draws,
        });
    }
    Ok((root, layers))
}
