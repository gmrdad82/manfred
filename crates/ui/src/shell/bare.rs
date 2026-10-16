use super::*;

impl Shell {
    pub fn bare(mut self) -> Shell {
        self.bare = true;
        self.strip = false;
        self.focus = Panel::C1;
        self
    }

    pub fn strip(mut self, on: bool) -> Shell {
        self.strip = on;
        self
    }

    pub fn is_bare(&self) -> bool {
        self.bare
    }

    pub(super) fn bare_view(&mut self, ui: &mut Ui, texture: &TextureHandle) {
        CentralPanel::default()
            .frame(Frame::NONE.fill(BG_DEEP))
            .show(ui, |ui| {
                let sense = if self.live() {
                    Sense::click_and_drag()
                } else {
                    Sense::hover()
                };
                let bar = if self.strip && self.span().is_some() {
                    transport::HEIGHT
                } else {
                    0.0
                };
                let room = (ui.available_size() - Vec2::new(0.0, bar)).max(Vec2::splat(1.0));
                let (area, response) = ui.allocate_exact_size(room, sense);
                let image = Rect::from_min_max(area.min.round(), area.max.round());
                self.viewport = Some(image);
                self.panels.push((Panel::C1, image));
                if !self.live() {
                    ui.painter().image(
                        texture.id(),
                        image,
                        viewport::cover(image, viewport::ASPECT),
                        Color32::WHITE,
                    );
                    return;
                }
                self.steer(ui, &response, image);
                self.picture(ui, image);
                if bar > 0.0 {
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            Pos2::new(image.left(), image.bottom()),
                            ui.max_rect().max,
                        ),
                        CornerRadius::ZERO,
                        BG_BASE,
                    );
                    self.transport(ui);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_shell_gives_the_whole_portrait_frame_to_the_viewport() {
        let path = crate::scratch("bare-portrait");
        std::fs::write(
            &path,
            "manfred = 1\nname = \"bare\"\n\n[[object]]\nname = \"box\"\nkind = \"box\"\nsize = [0.2, 0.2, 0.2]\n",
        )
        .unwrap();
        let text = path.display().to_string();
        let ctx = Context::default();
        crate::theme::apply_theme(&ctx, &crate::theme::manfred());
        let script = pfx_editor_shell::Script::default();
        let mut bare = Shell::open(&text).without_recent().bare();
        pfx_editor_shell::drive(&ctx, [1080, 1920], &script, |ui| bare.ui(ui));
        assert_eq!(
            bare.viewport(),
            Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1080.0, 1920.0)))
        );
        assert_eq!(bare.focus(), Panel::C1);
        let mut panelled = Shell::open(&text).without_recent();
        pfx_editor_shell::drive(&ctx, [1080, 1920], &script, |ui| panelled.ui(ui));
        assert!(panelled.viewport().unwrap().width() < 540.0);
    }
}
