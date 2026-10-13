use super::*;

const IMAGE_EXTENSIONS: &[&str] = &["png", "bmp", "dib", "jpg", "jpeg", "gif", "tif", "tiff"];

impl PaintApp {
    /// Requests a new workspace tab after an image has been decoded successfully.
    pub(crate) fn request_open_document(
        &mut self,
        buffer: rspaint_core::PixelBuffer,
        path: std::path::PathBuf,
    ) {
        self.opened_document = Some((buffer, path));
    }

    pub(crate) fn take_opened_document(
        &mut self,
    ) -> Option<(rspaint_core::PixelBuffer, std::path::PathBuf)> {
        self.opened_document.take()
    }

    pub fn copy_selection(&mut self) {
        if let Some(pixels) = self.editor.copy_selection() {
            crate::clipboard::write_image(&pixels);
        }
    }

    pub fn paste_clipboard(&mut self) {
        self.editor.paste_clipboard(crate::clipboard::read_image());
    }

    pub fn paste_from_file(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                crate::i18n::tr(ctx, "Images"),
                &["png", "bmp", "dib", "jpg", "jpeg", "gif", "tif", "tiff"],
            )
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.error_message = Some(crate::i18n::message(
                    ctx,
                    "Failed to read file: {error}",
                    &[("error", &error.to_string())],
                ));
                return;
            }
        };
        match rspaint_core::io::decode_image(&bytes) {
            Ok(pixels) => {
                let count = (pixels.width() * pixels.height()) as usize;
                self.editor
                    .paste_pixels(pixels, vec![true; count], Vec::new(), false, "Paste");
            }
            Err(error) => {
                self.error_message = Some(crate::i18n::message(
                    ctx,
                    "Failed to open image: {error}",
                    &[("error", &error.to_string())],
                ))
            }
        }
    }

    pub fn copy_to_file(&mut self, ctx: &egui::Context) {
        let Some(pixels) = self.editor.selection_pixels().cloned() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(crate::i18n::tr(ctx, "PNG image"), &["png"])
            .add_filter(crate::i18n::tr(ctx, "BMP image"), &["bmp", "dib"])
            .set_file_name("selection.png")
            .save_file()
        else {
            return;
        };
        let format = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(rspaint_core::io::SaveFormat::from_extension)
            .unwrap_or(rspaint_core::io::SaveFormat::Png);
        match rspaint_core::io::encode(&pixels, format) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(&path, bytes) {
                    self.error_message = Some(crate::i18n::message(
                        ctx,
                        "Failed to save: {error}",
                        &[("error", &error.to_string())],
                    ));
                }
            }
            Err(error) => {
                self.error_message = Some(crate::i18n::message(
                    ctx,
                    "Failed to encode image: {error}",
                    &[("error", &error.to_string())],
                ))
            }
        }
    }

    pub fn cut_selection(&mut self) {
        if let Some(pixels) = self.editor.cut_selection() {
            crate::clipboard::write_image(&pixels);
        }
    }

    pub fn open_file_dialog(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(crate::i18n::tr(ctx, "Images"), IMAGE_EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        self.open_image_path(ctx, &path);
    }

    pub(crate) fn supports_image_path(path: &std::path::Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                IMAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            })
    }

    /// Shared by the native dialog and desktop file drops.
    pub(crate) fn open_image_path(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.error_message = Some(crate::i18n::message(
                    ctx,
                    "Failed to read file: {error}",
                    &[("error", &error.to_string())],
                ));
                return;
            }
        };
        match rspaint_core::io::decode_image(&bytes) {
            Ok(buffer) => self.request_open_document(buffer, path.to_path_buf()),
            Err(error) => {
                self.error_message = Some(crate::i18n::message(
                    ctx,
                    "Failed to open image: {error}",
                    &[("error", &error.to_string())],
                ))
            }
        }
    }

    pub fn save(&mut self, ctx: &egui::Context) {
        match self.editor.file_path.clone() {
            Some(path) => self.write_to(ctx, &path),
            None => self.save_as(ctx),
        }
    }

    pub fn save_as(&mut self, ctx: &egui::Context) {
        let default_name = format!("{}.png", self.editor.document.name);
        let Some(path) = rfd::FileDialog::new()
            .add_filter(crate::i18n::tr(ctx, "PNG image"), &["png"])
            .add_filter(crate::i18n::tr(ctx, "BMP image"), &["bmp", "dib"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.write_to(ctx, &path);
    }

    pub(super) fn write_to(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        let format = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(rspaint_core::io::SaveFormat::from_extension)
            .unwrap_or(rspaint_core::io::SaveFormat::Png);
        match rspaint_core::io::encode(&self.editor.document.buffer, format) {
            Ok(bytes) => match std::fs::write(path, bytes) {
                Ok(()) => {
                    self.editor.mark_saved(path.to_path_buf());
                }
                Err(error) => {
                    self.error_message = Some(crate::i18n::message(
                        ctx,
                        "Failed to save: {error}",
                        &[("error", &error.to_string())],
                    ))
                }
            },
            Err(error) => {
                self.error_message = Some(crate::i18n::message(
                    ctx,
                    "Failed to encode image: {error}",
                    &[("error", &error.to_string())],
                ))
            }
        }
    }
}
