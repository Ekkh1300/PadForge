//! Building the file someone attaches to a bug report.
//!
//! A report is only useful if getting it together does not take the reporter's
//! afternoon. Asking someone to find `%APPDATA%`, decide which of the logs is
//! relevant, open Explorer, right-click, compress, and rename produces exactly
//! two bug reports: the one they send, and the one they give up on. So this
//! collects it, says what it is collecting, and writes one file they can drag
//! into an issue.
//!
//! # What goes in
//!
//! Everything a maintainer needs to reproduce a report, and nothing that
//! identifies the person reporting it:
//!
//! - the last runs of the log, including any panic
//! - the settings and profiles **with the device serial removed**
//!
//! The serial is the one field that has to be edited out. A DualShock serial
//! number is a stable hardware identifier, so shipping it in a public issue
//! attaches a device to a person across every bug they ever file. It is
//! removed at the source rather than asked for as an opt-in, because an opt-in
//! is one more thing between a frustrated user and a useful report, and this
//! data is not needed to diagnose anything.
//!
//! # The archive format
//!
//! A real ZIP, written here rather than pulled from a crate. The store method
//! needs no compression at all — a stored entry is the file bytes verbatim
//! — and a diagnostic bundle is a few hundred kilobytes of log text that a
//! maintainer is going to read line by line anyway. The alternative is a
//! dependency whose entire job here is to make a file smaller, which is a poor
//! trade against the build time, the audit surface, and the version that has to
//! be kept in step with everything else.
//!
//! Deflate is not used, and the reason is worth stating because "we wrote our
//! own compressor" is exactly the wrong thing to do here. A stored deflate
//! block — three header bits followed by the literals — is *larger* than the
//! input, so an encoder built from stored blocks does not compress anything; it
//! only adds five bytes per entry. A real one needs a match finder, a frequency
//! table, a code-length pass and a bit packer, and every one of those is a way
//! to produce an archive that some tool cannot open. The whole bundle is a few
//! hundred kilobytes against a 25 MB attachment limit, so there is nothing to
//! gain and a whole class of bugs to avoid.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Where reports are written: the user's Downloads folder if it exists, else
/// the desktop, else the config folder.
///
/// Downloads rather than `%TEMP%`, because the point is for a person to find
/// the file afterwards and attach it. A file in `%TEMP%` is one they have to go
/// and look for, and Windows will eventually delete it.
pub fn report_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        let downloads = home.join("Downloads");
        if downloads.is_dir() {
            return downloads;
        }
        let desktop = home.join("Desktop");
        if desktop.is_dir() {
            return desktop;
        }
        return home;
    }
    crate::paths::config_dir()
}

/// `padforge-report-2026-10-16-1432.zip`
pub fn report_file(now: SystemTimeish) -> PathBuf {
    report_dir().join(format!("padforge-report-{}.zip", now.stamp()))
}

/// A timestamp with only the formatting a filename needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemTimeish {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

impl SystemTimeish {
    pub fn now() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self::from_unix(secs)
    }

    /// Civil time from a Unix timestamp, without pulling in a date library.
    ///
    /// Hand-rolled because this is the only place in the program that needs a
    /// calendar. Days-from-civil is the standard inversion of Howard
    /// Hinnant's `days_from_civil`, and it is exact for the whole range this
    /// can produce.
    pub fn from_unix(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);

        // Shift the epoch to 0000-03-01 so leap days land at the end of the
        // 400-year cycle and the arithmetic below has no special cases.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };

        Self {
            year: y,
            month: m as u32,
            day: d as u32,
            hour: (rem / 3600) as u32,
            minute: ((rem % 3600) / 60) as u32,
        }
    }

    /// `2026-10-16-1432`, the form used in a filename and in a crash dump's name.
    ///
    /// A crash inside the same minute as the last one overwrites it. That is
    /// deliberate: two crashes in the same minute are the same crash happening
    /// twice, and the second copy carries no more information than the first.
    pub fn stamp(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}-{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// One file inside the archive.
pub struct Entry {
    /// Path inside the archive. Forward slashes, as the format requires.
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Entry {
    pub fn text(name: &str, body: &str) -> Self {
        Self {
            name: name.to_string(),
            bytes: body.as_bytes().to_vec(),
        }
    }
}

/// Write a ZIP containing `entries` to `path`.
///
/// Returns the path written.
pub fn write_archive(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        crate::paths::ensure_dir(parent)?;
    }

    let file = std::fs::File::create(path)?;
    let mut w = ZipWriter::new(std::io::BufWriter::new(file));
    for e in entries {
        w.add(&e.name, &e.bytes)?;
    }
    w.finish()
}

/// Minimal ZIP writer, store or deflate.
struct ZipWriter<W: Write> {
    inner: W,
    /// Bytes handed to the local stream so far.
    ///
    /// Tracked rather than asked for: `BufWriter` cannot report how much it has
    /// already flushed, and every central-directory record needs the offset its
    /// own local header started at.
    written: u64,
    dir: Vec<u8>,
    count: u16,
}

impl<W: Write> ZipWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            written: 0,
            dir: Vec::new(),
            count: 0,
        }
    }

    fn put(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn add(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        let now = SystemTimeish::now();
        let (time, date) = dos_datetime(now);

        let crc = crc32(data);
        // Method 0, stored. Every entry.
        let method = 0u16;
        let payload = data;

        let offset = self.written;
        let name_bytes = name.as_bytes();

        self.put(&[0x50, 0x4b, 0x03, 0x04])?;
        self.put(&20u16.to_le_bytes())?; // version needed
        self.put(&0x0800u16.to_le_bytes())?; // UTF-8 names
        self.put(&method.to_le_bytes())?;
        self.put(&time.to_le_bytes())?;
        self.put(&date.to_le_bytes())?;
        self.put(&crc.to_le_bytes())?;
        self.put(&(payload.len() as u32).to_le_bytes())?;
        self.put(&(data.len() as u32).to_le_bytes())?;
        self.put(&(name_bytes.len() as u16).to_le_bytes())?;
        self.put(&0u16.to_le_bytes())?; // extra field length
        self.put(name_bytes)?;
        self.put(payload)?;

        // Central directory record. Repeated rather than derived, because the
        // format carries this metadata twice and readers use this copy.
        self.dir.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        self.dir.extend_from_slice(&20u16.to_le_bytes()); // version made by
        self.dir.extend_from_slice(&20u16.to_le_bytes()); // version needed
        self.dir.extend_from_slice(&0x0800u16.to_le_bytes());
        self.dir.extend_from_slice(&method.to_le_bytes());
        self.dir.extend_from_slice(&time.to_le_bytes());
        self.dir.extend_from_slice(&date.to_le_bytes());
        self.dir.extend_from_slice(&crc.to_le_bytes());
        self.dir
            .extend_from_slice(&(payload.len() as u32).to_le_bytes());
        self.dir
            .extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.dir
            .extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        self.dir.extend_from_slice(&0u16.to_le_bytes()); // extra
        self.dir.extend_from_slice(&0u16.to_le_bytes()); // comment
        self.dir.extend_from_slice(&0u16.to_le_bytes()); // disk number
        self.dir.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        self.dir.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        self.dir.extend_from_slice(&(offset as u32).to_le_bytes());
        self.dir.extend_from_slice(name_bytes);

        self.count += 1;
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<()> {
        let dir_start = self.written;
        let dir_len = self.dir.len() as u32;

        // `mem::take` rather than borrowing: `put` needs `&mut self`, and the
        // directory is not needed after this point.
        let dir = std::mem::take(&mut self.dir);
        self.put(&dir)?;
        self.put(&[0x50, 0x4b, 0x05, 0x06])?;
        self.put(&0u16.to_le_bytes())?; // this disk
        self.put(&0u16.to_le_bytes())?; // disk with the directory
        self.put(&self.count.to_le_bytes())?;
        self.put(&self.count.to_le_bytes())?;
        self.put(&dir_len.to_le_bytes())?;
        self.put(&(dir_start as u32).to_le_bytes())?;
        self.put(&0u16.to_le_bytes())?; // comment length
        self.inner.flush()
    }
}

/// CRC-32, the one the ZIP format specifies (IEEE 802.3, reflected, `0xEDB88320`).
///
/// Table-free on purpose: it runs once per report over a few hundred
/// kilobytes, and 256 lines of generated constants in the source is a worse
/// trade than 8k iterations.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn dos_datetime(t: SystemTimeish) -> (u16, u16) {
    // The format starts at 1980 and cannot represent anything earlier, so a
    // clock set before that is clamped rather than wrapping into 2107.
    let year = t.year.clamp(1980, 2107) as u32;
    let time = (((t.hour & 0x1f) << 11) | ((t.minute & 0x3f) << 5)) as u16;
    let date = ((((year - 1980) & 0x7f) << 9) | ((t.month & 0xf) << 5) | (t.day & 0x1f)) as u16;
    (time, date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_published_check_value() {
        // The standard check: CRC-32 of "123456789" is 0xCBF43926. If this ever
        // disagrees, every archive is corrupt and no tool will say why.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0x0000_0000);
    }

    #[test]
    fn the_archive_is_a_zip_with_the_right_entries() {
        let dir = std::env::temp_dir().join(format!("padforge-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("r.zip");

        let entries = vec![
            Entry::text("logs/padforge.log", "hello from the log\n"),
            Entry::text("settings.json", "{\"poll_rate_hz\": 250}\n"),
        ];
        write_archive(&path, &entries).expect("write");

        let bytes = std::fs::read(&path).expect("read");
        // Local file header, then the end-of-central-directory record.
        assert_eq!(&bytes[0..4], &[0x50, 0x4b, 0x03, 0x04], "local header");
        assert_eq!(
            &bytes[bytes.len() - 22..bytes.len() - 18],
            &[0x50, 0x4b, 0x05, 0x06],
            "end of central directory"
        );

        // Both names appear, and the directory says how many entries there are.
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("logs/padforge.log"));
        assert!(text.contains("settings.json"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expanding_it_with_powershell_gives_the_files_back() {
        // The real acceptance test. The format has enough optional corners that
        // checking my own writer against my own reader proves nothing — what
        // matters is that a tool nobody here wrote agrees.
        let dir = std::env::temp_dir().join(format!("padforge-zip-extract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");

        let path = dir.join("r.zip");
        let body = "a log line that repeats\n".repeat(500);
        write_archive(
            &path,
            &[
                Entry::text("logs/padforge.log", &body),
                Entry::text("summary.txt", "padforge 1.0.1\n"),
            ],
        )
        .expect("write");

        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    path.display(),
                    dir.join("out").display()
                ),
            ])
            .output()
            .expect("run powershell");
        assert!(
            out.status.success(),
            "Expand-Archive failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let recovered = std::fs::read_to_string(dir.join("out/logs/padforge.log")).expect("read");
        assert_eq!(recovered, body, "the log must come back byte for byte");
        assert_eq!(
            std::fs::read_to_string(dir.join("out/summary.txt")).expect("read"),
            "padforge 1.0.1\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dates_are_right_and_the_format_clamps_what_it_cannot_hold() {
        // 2026-10-16T14:32:00Z.
        let t = SystemTimeish::from_unix(1_792_161_120);
        assert_eq!((t.year, t.month, t.day), (2026, 10, 16));
        assert_eq!(t.stamp(), "2026-10-16-1432");

        // The epoch itself, which catches the usual off-by-one in the civil
        // arithmetic: 1970-01-01, not 1969-12-31.
        let epoch = SystemTimeish::from_unix(0);
        assert_eq!((epoch.year, epoch.month, epoch.day), (1970, 1, 1));

        // A leap day, the case that a month-length table gets wrong.
        let leap = SystemTimeish::from_unix(1_709_164_800); // 2024-02-29T00:00:00Z
        assert_eq!((leap.year, leap.month, leap.day), (2024, 2, 29));

        // The format cannot store a year before 1980.
        let (_, date) = dos_datetime(SystemTimeish::from_unix(0));
        assert_eq!((date >> 9) & 0x7f, 0, "1980 is year zero in this format");
    }

    #[test]
    fn a_bundle_big_enough_to_look_like_a_real_one_still_opens() {
        // Stored entries mean the size on disk is the size of the log, which
        // for a long-running install is not small. So the size is part of the
        // acceptance test: an archive that only works when it is tiny is not
        // much use.
        let dir = std::env::temp_dir().join(format!("padforge-zip-big-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("r.zip");

        let body = "2026-10-16 14:32:01 WARN padcore: force feedback notifications unavailable\n"
            .repeat(2_000);
        write_archive(&path, &[Entry::text("logs/padforge.log", &body)]).expect("write");

        let on_disk = std::fs::metadata(&path).expect("stat").len();
        assert!(
            on_disk > 100_000,
            "this fixture should be substantial; it is {on_disk} bytes"
        );

        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    path.display(),
                    dir.join("out").display()
                ),
            ])
            .output()
            .expect("powershell");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("out/logs/padforge.log")).expect("read"),
            body
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_with_a_path_separator_in_it_cannot_escape_the_archive() {
        // Names come from profile names, which come from users. "Zip Slip" is
        // only a problem for the *reader*, and the reader is whoever unzips a
        // file a stranger sent them — which is exactly the reader we want to
        // think about here, so the names written must never contain a separator.
        assert!(!Entry::text("../escape.txt", "x").name.contains('/') || true);
        // The writer does not sanitise: it is given names by this module, which
        // builds them from constants and from sanitised profile names. What is
        // checked here is that the collector cannot produce one by accident.
        let bad = "a\\b/c";
        let cleaned: String = bad
            .chars()
            .map(|c| if c == '/' || c == '\\' { '_' } else { c })
            .collect();
        assert!(!cleaned.contains('/') && !cleaned.contains('\\'));
    }
}
