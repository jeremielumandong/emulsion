//! Portal owns copies, duplex and device options; Emulsion owns the sheet artwork.
use super::*;
use ashpd::desktop::print::{
    PageSet, PageSetup, PreparePrintOptions, PrintOptions, PrintPages, PrintProxy,
    Settings as PortalSettings,
};

pub struct Prepared {
    proxy: PrintProxy,
    token: u32,
    pub paper: Paper,
    pub landscape: bool,
    pub summary: String,
    pub copies: u16,
    pub grayscale: bool,
}
pub fn required() -> bool {
    Path::new("/.flatpak-info").exists() || std::env::var_os("FLATPAK_ID").is_some()
}
pub fn prepare(s: &Settings) -> Result<Prepared> {
    pollster::block_on(async {
        let proxy = PrintProxy::new().await?;
        let setup = PageSetup::default()
            .set_name(s.paper.id.as_str())
            .set_display_name(s.paper.name.as_str())
            .set_width(s.paper.width)
            .set_height(s.paper.height);
        let options = PreparePrintOptions::default()
            .set_supported_output_file_formats([ashpd::desktop::print::OutputFileFormat::Pdf]);
        let response = proxy
            .prepare_print(
                None,
                "Emulsion — choose printer and paper",
                PortalSettings::default()
                    .set_n_copies(1)
                    .set_scale(100)
                    .set_number_up(1),
                setup,
                options,
            )
            .await?
            .response()?;
        let r = response.settings;
        let p = response.page_setup;
        if r.scale.is_some_and(|v| v != 100)
            || r.number_up.is_some_and(|v| v != 1)
            || r.print_pages.is_some_and(|v| v != PrintPages::All)
            || r.page_set.is_some_and(|v| v != PageSet::All)
            || r.reverse == Some(true)
        {
            bail!(
                "Choose All pages, normal page order, 1 page per sheet and 100% scale in system settings; Emulsion composes the print layout."
            )
        }
        let landscape = matches!(
            p.orientation,
            Some(
                ashpd::desktop::print::Orientation::Landscape
                    | ashpd::desktop::print::Orientation::ReverseLandscape
            )
        );
        if matches!(
            p.orientation,
            Some(
                ashpd::desktop::print::Orientation::ReverseLandscape
                    | ashpd::desktop::print::Orientation::ReversePortrait
            )
        ) {
            bail!(
                "Use Portrait or Landscape in system print settings, without reverse orientation."
            )
        }
        let paper = Paper {
            id: p.name.unwrap_or_else(|| "system".into()),
            name: p.display_name.unwrap_or_else(|| "System paper".into()),
            width: p.width.context("System did not return paper width")?,
            height: p.height.context("System did not return paper height")?,
            margins: [
                p.margin_top.context("Missing printer margins")?,
                p.margin_right.context("Missing printer margins")?,
                p.margin_bottom.context("Missing printer margins")?,
                p.margin_left.context("Missing printer margins")?,
            ],
        };
        Ok(Prepared {
            proxy,
            token: response.token,
            paper,
            landscape,
            copies: u16::try_from(r.n_copies.unwrap_or(1)).context("Too many copies")?,
            grayscale: r.use_color == Some(false),
            summary: format!(
                "System printer settings · {} copies · {:?}",
                r.n_copies.unwrap_or(1),
                r.duplex
            ),
        })
    })
}
pub fn submit(prepared: &Prepared, title: &str, path: &Path) -> Result<String> {
    pollster::block_on(async {
        let file = std::fs::File::open(path)?;
        prepared
            .proxy
            .print(
                None,
                title,
                &file,
                PrintOptions::default().set_token(prepared.token),
            )
            .await?
            .response()?;
        Ok("Handed to the system print dialog. Check the system queue for progress.".into())
    })
}
