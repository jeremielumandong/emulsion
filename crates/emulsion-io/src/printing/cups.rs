//! CUPS 2 destination APIs, shared by Linux and macOS. Loaded dynamically so a
//! missing print service/library never prevents the editor or PDF export opening.
use super::*;
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_uint, c_void},
    ptr,
};
type Ptr = *mut c_void;
#[repr(C)]
struct OptionPair {
    name: *mut c_char,
    value: *mut c_char,
}
#[repr(C)]
struct Dest {
    name: *mut c_char,
    instance: *mut c_char,
    is_default: c_int,
    num_options: c_int,
    options: *mut OptionPair,
}
#[repr(C)]
struct Media {
    name: [c_char; 128],
    width: c_int,
    height: c_int,
    bottom: c_int,
    left: c_int,
    right: c_int,
    top: c_int,
}
impl Default for Media {
    fn default() -> Self {
        Self {
            name: [0; 128],
            width: 0,
            height: 0,
            bottom: 0,
            left: 0,
            right: 0,
            top: 0,
        }
    }
}
macro_rules! api {
    ($($name:ident: $ty:ty),* $(,)?)=>{
        #[allow(non_snake_case)] struct Api { $($name:$ty,)* _library:Library }
        impl Api {fn load()->Result<&'static Self>{
            // Keep libcups loaded: its thread-local cleanup callbacks can outlive a query.
            static API: std::sync::OnceLock<Api> = std::sync::OnceLock::new();
            if let Some(api)=API.get(){return Ok(api)}
            // SAFETY: fixed OS library paths and signatures from cups/cups.h.
            unsafe {
                #[cfg(target_os="macos")] let library=Library::new("/usr/lib/libcups.2.dylib")?;
                #[cfg(target_os="linux")] let library=Library::new("libcups.so.2").context("CUPS client library is unavailable. Install libcups or use Save PDF.")?;
                let api=Self{$($name:*library.get(concat!(stringify!($name),"\0").as_bytes())?,)*_library:library};
                Ok(API.get_or_init(||api))
            }
        }}
    }
}
api! {
 cupsGetDests2:unsafe extern "C" fn(Ptr,*mut *mut Dest)->c_int,
 cupsFreeDests:unsafe extern "C" fn(c_int,*mut Dest),
 cupsGetNamedDest:unsafe extern "C" fn(Ptr,*const c_char,*const c_char)->*mut Dest,
 cupsGetOption:unsafe extern "C" fn(*const c_char,c_int,*mut OptionPair)->*const c_char,
 cupsLastError:unsafe extern "C" fn()->c_int,
 cupsLastErrorString:unsafe extern "C" fn()->*const c_char,
 cupsConnectDest:unsafe extern "C" fn(*mut Dest,c_uint,c_int,*mut c_int,*mut c_char,usize,Ptr,Ptr)->Ptr,
 httpClose:unsafe extern "C" fn(Ptr),
 cupsCopyDestInfo:unsafe extern "C" fn(Ptr,*mut Dest)->Ptr,
 cupsFreeDestInfo:unsafe extern "C" fn(Ptr),
 cupsGetDestMediaCount:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,c_uint)->c_int,
 cupsGetDestMediaByIndex:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,c_int,c_uint,*mut Media)->c_int,
 cupsGetDestMediaDefault:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,c_uint,*mut Media)->c_int,
 cupsGetDestMediaByName:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,*const c_char,c_uint,*mut Media)->c_int,
 cupsLocalizeDestValue:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,*const c_char,*const c_char)->*const c_char,
 cupsLocalizeDestMedia:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,c_uint,*mut Media)->*const c_char,
 cupsFindDestSupported:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,*const c_char)->Ptr,
 cupsCheckDestSupported:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,*const c_char,*const c_char)->c_int,
 ippGetCount:unsafe extern "C" fn(Ptr)->c_int,
 ippGetString:unsafe extern "C" fn(Ptr,c_int,*mut *const c_char)->*const c_char,
 ippGetInteger:unsafe extern "C" fn(Ptr,c_int)->c_int,
 cupsAddOption:unsafe extern "C" fn(*const c_char,*const c_char,c_int,*mut *mut OptionPair)->c_int,
 cupsAddDestMediaOptions:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,c_uint,*mut Media,c_int,*mut *mut OptionPair)->c_int,
 cupsFreeOptions:unsafe extern "C" fn(c_int,*mut OptionPair),
 cupsCopyDestConflicts:unsafe extern "C" fn(Ptr,*mut Dest,Ptr,c_int,*mut OptionPair,*const c_char,*const c_char,*mut c_int,*mut *mut OptionPair,*mut c_int,*mut *mut OptionPair)->c_int,
 cupsPrintFile2:unsafe extern "C" fn(Ptr,*const c_char,*const c_char,*const c_char,c_int,*mut OptionPair)->c_int,
}
fn string(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: only called on CUPS-owned, NUL-terminated strings while their owner lives.
        unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}
fn cs(s: &str) -> Result<CString> {
    Ok(CString::new(s)?)
}
struct Connection<'a> {
    api: &'a Api,
    dest: *mut Dest,
    http: Ptr,
    info: Ptr,
}
impl Drop for Connection<'_> {
    fn drop(&mut self) {
        // SAFETY: these resources were acquired from the same live CUPS library, once.
        unsafe {
            if !self.info.is_null() {
                (self.api.cupsFreeDestInfo)(self.info)
            }
            if !self.http.is_null() {
                (self.api.httpClose)(self.http)
            }
            if !self.dest.is_null() {
                (self.api.cupsFreeDests)(1, self.dest)
            }
        }
    }
}
impl<'a> Connection<'a> {
    fn open(api: &'a Api, name: &str) -> Result<Self> {
        let name = cs(name)?;
        // SAFETY: inputs outlive each synchronous FFI call; every nullable result is checked.
        unsafe {
            let mut c = Self {
                api,
                dest: (api.cupsGetNamedDest)(ptr::null_mut(), name.as_ptr(), ptr::null()),
                http: ptr::null_mut(),
                info: ptr::null_mut(),
            };
            if c.dest.is_null() {
                bail!(
                    "Printer is unavailable: {}",
                    string((api.cupsLastErrorString)())
                )
            }
            let mut resource = [0 as c_char; 1024];
            c.http = (api.cupsConnectDest)(
                c.dest,
                0,
                5000,
                ptr::null_mut(),
                resource.as_mut_ptr(),
                resource.len(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            if c.http.is_null() {
                bail!(
                    "Could not connect to printer: {}",
                    string((api.cupsLastErrorString)())
                )
            }
            c.info = (api.cupsCopyDestInfo)(c.http, c.dest);
            if c.info.is_null() {
                bail!(
                    "Printer capabilities unavailable: {}",
                    string((api.cupsLastErrorString)())
                )
            }
            Ok(c)
        }
    }
    fn choices(&self, key: &str, integer: bool) -> Result<Vec<Choice>> {
        let key = cs(key)?;
        let a = self.api;
        // SAFETY: CUPS attribute belongs to info; it remains alive for this loop.
        unsafe {
            let attr = (a.cupsFindDestSupported)(self.http, self.dest, self.info, key.as_ptr());
            if attr.is_null() {
                return Ok(vec![]);
            }
            let n = (a.ippGetCount)(attr).clamp(0, 1024);
            Ok((0..n)
                .map(|i| {
                    let id = if integer {
                        (a.ippGetInteger)(attr, i).to_string()
                    } else {
                        string((a.ippGetString)(attr, i, ptr::null_mut()))
                    };
                    let localized = CString::new(id.as_str())
                        .ok()
                        .map(|v| {
                            string((a.cupsLocalizeDestValue)(
                                self.http,
                                self.dest,
                                self.info,
                                key.as_ptr(),
                                v.as_ptr(),
                            ))
                        })
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| id.clone());
                    let name = match id.as_str() {
                        "3" if integer => "Draft",
                        "4" if integer => "Normal",
                        "5" if integer => "High",
                        "one-sided" => "One-sided",
                        "two-sided-long-edge" => "Duplex · long edge",
                        "two-sided-short-edge" => "Duplex · short edge",
                        _ => &localized,
                    }
                    .to_owned();
                    Choice { id, name }
                })
                .filter(|c| !c.id.is_empty())
                .collect())
        }
    }
}
struct Options<'a> {
    api: &'a Api,
    count: c_int,
    ptr: *mut OptionPair,
}
impl Drop for Options<'_> {
    fn drop(&mut self) {
        // SAFETY: options is allocated by cupsAddOption/MediaOptions and owned here.
        unsafe { (self.api.cupsFreeOptions)(self.count, self.ptr) }
    }
}
impl Options<'_> {
    fn add(&mut self, key: &str, value: &str) -> Result<()> {
        let (k, v) = (cs(key)?, cs(value)?); // SAFETY: CUPS copies both strings; option storage is tracked by this owner.
        unsafe {
            self.count = (self.api.cupsAddOption)(k.as_ptr(), v.as_ptr(), self.count, &mut self.ptr)
        }
        Ok(())
    }
}

pub fn discover() -> Result<Vec<Printer>> {
    let api = Api::load()?;
    // SAFETY: returned array lives until cupsFreeDests below; length checked before slice creation.
    unsafe {
        let mut dests = ptr::null_mut();
        let n = (api.cupsGetDests2)(ptr::null_mut(), &mut dests);
        if n <= 0 || dests.is_null() {
            if !dests.is_null() {
                (api.cupsFreeDests)(n, dests)
            }
            if (api.cupsLastError)() >= 0x400 {
                bail!(
                    "Print service unavailable: {}",
                    string((api.cupsLastErrorString)())
                )
            }
            return Ok(vec![]);
        }
        let result = std::slice::from_raw_parts(dests, n as usize)
            .iter()
            .filter(|d| d.instance.is_null())
            .map(|d| {
                let value = |key: &CStr| {
                    string((api.cupsGetOption)(key.as_ptr(), d.num_options, d.options))
                };
                let id = string(d.name);
                let description = value(c"printer-info");
                let status = match value(c"printer-state").as_str() {
                    "3" => "Ready",
                    "4" => "Printing",
                    "5" => "Stopped",
                    _ => "Status unknown",
                };
                Printer {
                    name: if description.is_empty() {
                        id.clone()
                    } else {
                        description
                    },
                    id,
                    default: d.is_default != 0,
                    status: status.into(),
                }
            })
            .collect();
        (api.cupsFreeDests)(n, dests);
        Ok(result)
    }
}
fn paper(m: &Media, name: String) -> Paper {
    Paper {
        id: string(m.name.as_ptr()),
        name,
        width: m.width as f64 / 100.,
        height: m.height as f64 / 100.,
        margins: [m.top, m.right, m.bottom, m.left].map(|v| v as f64 / 100.),
    }
}
pub fn capabilities(name: &str) -> Result<Capabilities> {
    let a = Api::load()?;
    let c = Connection::open(a, name)?;
    let mut papers = vec![];
    // SAFETY: output media structures have the exact public CUPS ABI and are initialized.
    unsafe {
        let mut default = Media::default();
        let has_default = (a.cupsGetDestMediaDefault)(c.http, c.dest, c.info, 0, &mut default) != 0;
        for flags in [0, 1] {
            let n = (a.cupsGetDestMediaCount)(c.http, c.dest, c.info, flags).clamp(0, 1024);
            for i in 0..n {
                let mut m = Media::default();
                if (a.cupsGetDestMediaByIndex)(c.http, c.dest, c.info, i, flags, &mut m) == 0 {
                    continue;
                }
                let label = string((a.cupsLocalizeDestMedia)(
                    c.http, c.dest, c.info, flags, &mut m,
                ));
                let p = paper(
                    &m,
                    if label.is_empty() {
                        string(m.name.as_ptr())
                    } else {
                        label
                    },
                );
                if !papers.iter().any(|old: &Paper| old.id == p.id) {
                    papers.push(p)
                }
            }
        }
        if papers.is_empty() {
            bail!("The printer did not report supported paper sizes")
        }
        let color = c
            .choices("print-color-mode", false)?
            .iter()
            .any(|v| v.id == "color" || v.id == "auto");
        Ok(Capabilities {
            default_paper: if has_default {
                string(default.name.as_ptr())
            } else {
                papers[0].id.clone()
            },
            papers,
            media: c.choices("media-type", false)?,
            trays: c.choices("media-source", false)?,
            quality: c.choices("print-quality", true)?,
            sides: c.choices("sides", false)?,
            color,
        })
    }
}
fn job_options<'a>(a: &'a Api, c: &Connection<'_>, s: &Settings) -> Result<Options<'a>> {
    let mut options = Options {
        api: a,
        count: 0,
        ptr: ptr::null_mut(),
    };
    // SAFETY: all CUPS objects are scoped to this function and outputs have owners.
    unsafe {
        let accepting = string((a.cupsGetOption)(
            c"printer-is-accepting-jobs".as_ptr(),
            (*c.dest).num_options,
            (*c.dest).options,
        ));
        if accepting == "false" {
            bail!("The printer is not accepting jobs")
        }
        let media_name = cs(&s.paper.id)?;
        let mut media = Media::default();
        if (a.cupsGetDestMediaByName)(c.http, c.dest, c.info, media_name.as_ptr(), 0, &mut media)
            == 0
        {
            bail!("Selected paper is no longer supported; refresh printer settings")
        }
        let live = paper(&media, s.paper.name.clone());
        if live != s.paper {
            bail!("Printer paper dimensions changed; refresh printer settings")
        }
        options.count = (a.cupsAddDestMediaOptions)(
            c.http,
            c.dest,
            c.info,
            0,
            &mut media,
            options.count,
            &mut options.ptr,
        );
        for (key, value) in [
            ("media-type", s.media.as_deref()),
            ("media-source", s.tray.as_deref()),
            ("print-quality", s.quality.as_deref()),
            ("sides", s.sides.as_deref()),
        ] {
            if let Some(value) = value {
                let (k, v) = (cs(key)?, cs(value)?);
                if (a.cupsCheckDestSupported)(c.http, c.dest, c.info, k.as_ptr(), v.as_ptr()) == 0 {
                    bail!("The printer no longer supports {key}={value}")
                }
                options.add(key, value)?
            }
        }
        options.add("copies", &s.copies.to_string())?;
        options.add(
            "multiple-document-handling",
            "separate-documents-collated-copies",
        )?;
        options.add("print-scaling", "none")?;
        options.add("number-up", "1")?;
        options.add("orientation-requested", if s.landscape { "4" } else { "3" })?;
        // Grayscale is composed in the spool document, so it works even without a driver color option.
        let mode = if s.grayscale { "monochrome" } else { "color" };
        let v = cs(mode)?;
        if (a.cupsCheckDestSupported)(
            c.http,
            c.dest,
            c.info,
            c"print-color-mode".as_ptr(),
            v.as_ptr(),
        ) != 0
        {
            options.add("print-color-mode", mode)?
        }
        let mut conflicts = Options {
            api: a,
            count: 0,
            ptr: ptr::null_mut(),
        };
        let mut resolved = Options {
            api: a,
            count: 0,
            ptr: ptr::null_mut(),
        };
        let conflict = (a.cupsCopyDestConflicts)(
            c.http,
            c.dest,
            c.info,
            options.count,
            options.ptr,
            ptr::null(),
            ptr::null(),
            &mut conflicts.count,
            &mut conflicts.ptr,
            &mut resolved.count,
            &mut resolved.ptr,
        );
        if conflict != 0 || conflicts.count > 0 {
            bail!(
                "This paper, media, tray and duplex combination conflicts. Adjust printer settings."
            )
        }
        Ok(options)
    }
}
pub fn validate(name: &str, settings: &Settings) -> Result<()> {
    let api = Api::load()?;
    let connection = Connection::open(api, name)?;
    job_options(api, &connection, settings)?;
    Ok(())
}
pub fn submit(
    name: &str,
    title: &str,
    path: &Path,
    s: &Settings,
    cancel: &AtomicBool,
) -> Result<String> {
    let a = Api::load()?;
    let c = Connection::open(a, name)?;
    let options = job_options(a, &c, s)?;
    canceled(cancel)?;
    // SAFETY: owned strings and CUPS options remain valid throughout submission.
    unsafe {
        let (name, title, file) = (
            cs(name)?,
            cs(title)?,
            cs(path.to_str().context("Invalid spool path")?)?,
        );
        let id = (a.cupsPrintFile2)(
            ptr::null_mut(),
            name.as_ptr(),
            file.as_ptr(),
            title.as_ptr(),
            options.count,
            options.ptr,
        );
        if id <= 0 {
            bail!(
                "Print submission failed or its outcome is unknown: {}. Check the system queue before retrying.",
                string((a.cupsLastErrorString)())
            )
        }
        Ok(format!(
            "Sent to {} · job {id}. Check the system print queue for progress.",
            name.to_string_lossy()
        ))
    }
}
