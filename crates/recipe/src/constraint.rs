use crate::{Camera, Constrained, Constraint, ConstraintKind, Cycle, Owner, Recipe, Rig};

impl Recipe {
    pub fn constrained(&self) -> Vec<Constrained> {
        let mut out: Vec<Constrained> = Vec::new();
        let mut add = |owner: Owner, mut list: Vec<Constraint>| {
            if out.iter().any(|c| c.owner == owner) {
                return;
            }
            list.extend(
                self.constraints
                    .iter()
                    .filter(|c| c.owner == owner)
                    .flat_map(|c| c.list.iter().cloned()),
            );
            if !list.is_empty() {
                out.push(Constrained { owner, list });
            }
        };
        for object in &self.objects {
            let sugar = self
                .follows
                .iter()
                .enumerate()
                .filter(|(_, f)| f.object == object.name)
                .map(|(i, f)| Constraint {
                    name: None,
                    path: format!("follow[{}]", f.object),
                    ipath: format!("follow#{i}"),
                    kind: ConstraintKind::FollowPath(f.clone()),
                })
                .collect();
            add(Owner::Object(object.name.clone()), sugar);
        }
        for (i, camera) in self.cameras.iter().enumerate() {
            let sugar = camera
                .rig
                .iter()
                .map(|rig| Constraint {
                    name: None,
                    path: format!("camera[{}].rig", camera.name),
                    ipath: format!("camera#{i}.rig"),
                    kind: ConstraintKind::Rig(Box::new(rig.clone())),
                })
                .collect();
            add(Owner::Camera(camera.name.clone()), sugar);
        }
        for light in &self.lights {
            add(Owner::Light(light.name.clone()), Vec::new());
        }
        out
    }

    pub fn target_owner(&self, target: &str) -> Option<Owner> {
        if self.objects.iter().any(|o| o.name == target) {
            return Some(Owner::Object(target.to_string()));
        }
        if let Some(name) = target.strip_prefix("camera.")
            && self.cameras.iter().any(|c| c.name == name)
        {
            return Some(Owner::Camera(name.to_string()));
        }
        if let Some(name) = target.strip_prefix("light.")
            && self.lights.iter().any(|l| l.name == name)
        {
            return Some(Owner::Light(name.to_string()));
        }
        self.objects
            .iter()
            .filter(|o| {
                target
                    .strip_prefix(o.name.as_str())
                    .is_some_and(|rest| rest.len() > 1 && rest.starts_with('.'))
            })
            .max_by_key(|o| o.name.len())
            .map(|o| Owner::Object(o.name.clone()))
    }

    pub fn rig_of<'a>(&'a self, camera: &'a Camera) -> Option<&'a Rig> {
        camera.rig.as_ref().or_else(|| {
            self.constraints
                .iter()
                .filter(|c| c.owner == Owner::Camera(camera.name.clone()))
                .flat_map(|c| &c.list)
                .find_map(|c| match &c.kind {
                    ConstraintKind::Rig(rig) => Some(&**rig),
                    _ => None,
                })
        })
    }

    pub fn constraint_order(&self, all: &[Constrained]) -> Result<Vec<usize>, Cycle> {
        let needs: Vec<Vec<(usize, usize)>> = all
            .iter()
            .map(|c| {
                c.list
                    .iter()
                    .enumerate()
                    .filter_map(|(k, constraint)| {
                        let (_, target) = constraint.kind.target()?;
                        let owner = self.target_owner(target)?;
                        let at = all.iter().position(|e| e.owner == owner)?;
                        Some((k, at))
                    })
                    .collect()
            })
            .collect();
        let mut walk = Walk {
            all,
            needs: &needs,
            state: vec![State::New; all.len()],
            trail: Vec::new(),
            order: Vec::with_capacity(all.len()),
        };
        for at in 0..all.len() {
            if walk.state[at] == State::New {
                walk.visit(at)?;
            }
        }
        Ok(walk.order)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    New,
    Open,
    Done,
}

struct Walk<'a> {
    all: &'a [Constrained],
    needs: &'a [Vec<(usize, usize)>],
    state: Vec<State>,
    trail: Vec<(usize, usize)>,
    order: Vec<usize>,
}

impl Walk<'_> {
    fn visit(&mut self, at: usize) -> Result<(), Cycle> {
        self.state[at] = State::Open;
        for &(k, next) in &self.needs[at] {
            self.trail.push((at, k));
            match self.state[next] {
                State::Open => return Err(self.cycle(next)),
                State::New => self.visit(next)?,
                State::Done => {}
            }
            self.trail.pop();
        }
        self.state[at] = State::Done;
        self.order.push(at);
        Ok(())
    }

    fn cycle(&self, back: usize) -> Cycle {
        let start = self
            .trail
            .iter()
            .position(|(owner, _)| *owner == back)
            .unwrap_or(0);
        let steps: Vec<String> = self.trail[start..]
            .iter()
            .map(|&(owner, k)| {
                let constraint = &self.all[owner].list[k];
                let target = constraint.kind.target().map_or("", |(_, t)| t);
                format!(
                    "`{}` {} `{target}`",
                    self.all[owner].owner.node(),
                    constraint.kind.verb()
                )
            })
            .collect();
        let (owner, k) = self.trail[self.trail.len() - 1];
        let constraint = &self.all[owner].list[k];
        let key = constraint.kind.target().map_or("target", |(key, _)| key);
        Cycle {
            path: format!("{}.{key}", constraint.path),
            ipath: format!("{}.{key}", constraint.ipath),
            text: format!(
                "the constraints go round in a cycle, {}, expected a target that doesn't lead back to `{}`",
                steps.join(", "),
                self.all[back].owner.node()
            ),
        }
    }
}
