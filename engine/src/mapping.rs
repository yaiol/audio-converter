//! What a field is called in each tag format - one table, Mp3tag's.
//!
//! The rows are the mapping page of Mp3tag (<https://docs.mp3tag.de/mapping/>), which was checked
//! against files Mp3tag itself wrote (2026-10-14): the ID3 frames, the RIFF INFO ids and the field
//! names all matched. Mp3tag is the reference because it is the tagger most people use, so these
//! are the names players and other taggers have learned to read.
//!
//! Everything that needs "what is this field called there" asks here: the tag layer that turns a
//! custom ID3 frame into the real one, the writer of MP4 atoms, the matching of a name typed in a
//! cleaning entry against whichever spelling a file uses, and the suggestions under that entry.
//! A field is in the table only when it has a home of its own in at least one format; a custom field
//! (`RELEASETYPE`) is not here and stays a custom field everywhere.

/// One field.
#[derive(Debug)]
pub struct Field {
    /// Mp3tag's name for it - also the name a Vorbis comment carries.
    pub name: &'static str,
    /// Other names it arrives under: ffmpeg's internal keys, and other taggers' spellings.
    pub aliases: &'static [&'static str],
    /// The ID3v2.3 frame, when the format has one.
    pub id3_v23: Option<&'static str>,
    /// The ID3v2.4 frame.
    pub id3_v24: Option<&'static str>,
    /// The MP4 atom: four characters, `\u{a9}` standing for the copyright sign (0xA9).
    pub mp4: Option<&'static str>,
    /// The WMA / ASF attribute name.
    pub asf: Option<&'static str>,
    /// The WAV `LIST INFO` id (four characters) ffmpeg reads and writes for it. Ids ffmpeg does not map
    /// (`ITRK`, `IWRI`...) are not here: they are dropped on a conversion, by design.
    pub riff: Option<&'static str>,
    /// The name a Vorbis comment (FLAC, Ogg Vorbis, Opus) or an APE tag (WavPack, TTA) gives it: the
    /// Mp3tag name, except the year (`DATE`) and the track (`TRACKNUMBER`).
    pub vorbis: &'static str,
}

const fn f(
    name: &'static str,
    aliases: &'static [&'static str],
    id3_v23: Option<&'static str>,
    id3_v24: Option<&'static str>,
    mp4: Option<&'static str>,
    asf: Option<&'static str>,
    riff: Option<&'static str>,
    vorbis: &'static str,
) -> Field {
    Field { name, aliases, id3_v23, id3_v24, mp4, asf, riff, vorbis }
}

pub const FIELDS: &[Field] = &[
    f("ALBUM", &[], Some("TALB"), Some("TALB"), Some("\u{a9}alb"), Some("WM/AlbumTitle"), Some("IPRD"), "ALBUM"),
    f("ALBUMSORT", &["album-sort", "sort_album"], Some("TSOA"), Some("TSOA"), Some("soal"), Some("WM/AlbumSortOrder"), None, "ALBUMSORT"),
    f("ALBUMARTIST", &["album_artist"], Some("TPE2"), Some("TPE2"), Some("aART"), Some("WM/AlbumArtist"), None, "ALBUMARTIST"),
    f("ALBUMARTISTSORT", &["sort_album_artist"], Some("TSO2"), Some("TSO2"), Some("soaa"), None, None, "ALBUMARTISTSORT"),
    f("ARTIST", &[], Some("TPE1"), Some("TPE1"), Some("\u{a9}ART"), Some("Author"), Some("IART"), "ARTIST"),
    f("ARTISTSORT", &["artist-sort", "sort_artist"], Some("TSOP"), Some("TSOP"), Some("soar"), Some("WM/ArtistSortOrder"), None, "ARTISTSORT"),
    f("BPM", &[], Some("TBPM"), Some("TBPM"), Some("tmpo"), Some("WM/BeatsPerMinute"), None, "BPM"),
    f("COMMENT", &["DESCRIPTION"], Some("COMM"), Some("COMM"), Some("\u{a9}cmt"), Some("Description"), Some("ICMT"), "COMMENT"),
    f("COMPILATION", &[], Some("TCMP"), Some("TCMP"), Some("cpil"), None, None, "COMPILATION"),
    f("COMPOSER", &[], Some("TCOM"), Some("TCOM"), Some("\u{a9}wrt"), Some("WM/Composer"), None, "COMPOSER"),
    f("COMPOSERSORT", &["sort_composer"], Some("TSOC"), Some("TSOC"), Some("soco"), None, None, "COMPOSERSORT"),
    f("CONDUCTOR", &[], Some("TPE3"), Some("TPE3"), Some("\u{a9}con"), Some("WM/Conductor"), None, "CONDUCTOR"),
    f("CONTENTGROUP", &["grouping"], Some("TIT1"), Some("TIT1"), Some("\u{a9}grp"), Some("WM/ContentGroupDescription"), None, "CONTENTGROUP"),
    f("COPYRIGHT", &[], Some("TCOP"), Some("TCOP"), Some("cprt"), Some("Copyright"), Some("ICOP"), "COPYRIGHT"),
    f("DISCNUMBER", &["disc"], Some("TPOS"), Some("TPOS"), Some("disk"), Some("WM/PartOfSet"), None, "DISCNUMBER"),
    f("ENCODEDBY", &["encoded_by"], Some("TENC"), Some("TENC"), Some("\u{a9}enc"), Some("WM/EncodedBy"), None, "ENCODEDBY"),
    f("ENCODERSETTINGS", &[], Some("TSSE"), Some("TSSE"), None, Some("WM/EncodingSettings"), None, "ENCODERSETTINGS"),
    f("GENRE", &[], Some("TCON"), Some("TCON"), Some("\u{a9}gen"), Some("WM/Genre"), Some("IGNR"), "GENRE"),
    f("INITIALKEY", &[], Some("TKEY"), Some("TKEY"), None, Some("WM/InitialKey"), None, "INITIALKEY"),
    f("ISRC", &[], Some("TSRC"), Some("TSRC"), None, Some("WM/ISRC"), None, "ISRC"),
    f("LANGUAGE", &[], Some("TLAN"), Some("TLAN"), None, Some("WM/Language"), Some("ILNG"), "LANGUAGE"),
    f("LYRICIST", &["WRITER"], Some("TEXT"), Some("TEXT"), None, Some("WM/Writer"), None, "LYRICIST"),
    f("MEDIATYPE", &["media_type"], Some("TMED"), Some("TMED"), None, None, None, "MEDIATYPE"),
    f("MIXARTIST", &[], Some("TPE4"), Some("TPE4"), None, None, None, "MIXARTIST"),
    f("MOOD", &[], None, Some("TMOO"), None, Some("WM/Mood"), None, "MOOD"),
    f("ORIGALBUM", &[], Some("TOAL"), Some("TOAL"), None, Some("WM/OriginalAlbumTitle"), None, "ORIGALBUM"),
    f("ORIGARTIST", &[], Some("TOPE"), Some("TOPE"), None, Some("WM/OriginalArtist"), None, "ORIGARTIST"),
    f("ORIGLYRICIST", &[], Some("TOLY"), Some("TOLY"), None, Some("WM/OriginalLyricist"), None, "ORIGLYRICIST"),
    // v2.4 holds the original release as a timestamp frame (TDOR), which is not a text frame.
    f("ORIGYEAR", &[], Some("TORY"), None, None, Some("WM/OriginalReleaseYear"), None, "ORIGYEAR"),
    // ORGANIZATION is the Xiph spelling of the label (every FLAC of the owner's library carries it, none
    // carries PUBLISHER), so it is the publisher in the formats that have a publisher field of their own.
    f("PUBLISHER", &["LABEL", "ORGANIZATION"], Some("TPUB"), Some("TPUB"), Some("\u{a9}pub"), Some("WM/Publisher"), None, "PUBLISHER"),
    f("SUBTITLE", &[], Some("TIT3"), Some("TIT3"), None, Some("WM/SubTitle"), None, "SUBTITLE"),
    f("TITLE", &[], Some("TIT2"), Some("TIT2"), Some("\u{a9}nam"), Some("Title"), Some("INAM"), "TITLE"),
    f("TITLESORT", &["title-sort", "sort_name"], Some("TSOT"), Some("TSOT"), Some("sonm"), Some("WM/TitleSortOrder"), None, "TITLESORT"),
    f("TRACK", &["TRACKNUMBER"], Some("TRCK"), Some("TRCK"), Some("trkn"), Some("WM/TrackNumber"), Some("IPRT"), "TRACKNUMBER"),
    f("UNSYNCEDLYRICS", &["USLT", "LYRICS"], Some("USLT"), Some("USLT"), Some("\u{a9}lyr"), Some("WM/Lyrics"), None, "UNSYNCEDLYRICS"),
    // The date: v2.3 splits it over TYER / TDAT / TIME, v2.4 has the one timestamp TDRC. Neither is a
    // text frame a custom one can be promoted to, so the entry exists for the names only.
    f("YEAR", &["DATE"], Some("TYER"), Some("TDRC"), Some("\u{a9}day"), Some("WM/Year"), Some("ICRD"), "DATE"),
];

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The field that `name` is a spelling of: its Mp3tag name, one of its aliases, or one of its frames.
pub fn find(name: &str) -> Option<&'static Field> {
    let name = name.trim();
    FIELDS.iter().find(|fd| {
        eq(fd.name, name)
            || fd.aliases.iter().any(|a| eq(a, name))
            || fd.id3_v23.is_some_and(|x| eq(x, name))
            || fd.id3_v24.is_some_and(|x| eq(x, name))
    })
}

/// A field known by its name or an alias only - not by a frame id, which is a different spelling
/// altogether and matches something else when it is a custom frame's description.
pub fn find_by_name(name: &str) -> Option<&'static Field> {
    let name = name.trim();
    FIELDS.iter().find(|fd| eq(fd.name, name) || fd.aliases.iter().any(|a| eq(a, name)))
}

/// Every spelling of the field `name` belongs to (empty when it is not in the table): its name,
/// aliases, frames. Used to match a name typed by a user against whatever a file calls it.
pub fn spellings(name: &str) -> Vec<String> {
    let Some(fd) = find(name) else { return Vec::new() };
    let mut v: Vec<String> = vec![fd.name.to_owned()];
    v.extend(fd.aliases.iter().map(|a| (*a).to_owned()));
    for id in [fd.id3_v23, fd.id3_v24].into_iter().flatten() {
        v.push(id.to_owned());
    }
    v.dedup_by(|a, b| eq(a, b));
    v
}

/// The four bytes of an MP4 atom name (`\u{a9}` becoming 0xA9).
pub fn atom_bytes(atom: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut n = 0;
    for ch in atom.chars() {
        if n >= 4 {
            return None;
        }
        out[n] = if ch == '\u{a9}' { 0xA9 } else if ch.is_ascii() { ch as u8 } else { return None };
        n += 1;
    }
    (n == 4).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vorbis_name_of_a_field_is_its_name_or_one_of_its_aliases() {
        for fd in FIELDS {
            assert!(eq(fd.vorbis, fd.name) || fd.aliases.iter().any(|a| eq(a, fd.vorbis)), "{}: {}", fd.name, fd.vorbis);
        }
        assert_eq!(find("year").unwrap().vorbis, "DATE");
        assert_eq!(find("track").unwrap().vorbis, "TRACKNUMBER");
    }

    #[test]
    fn a_field_is_found_by_its_name_an_alias_or_a_frame() {
        assert_eq!(find("lyricist").unwrap().name, "LYRICIST");
        assert_eq!(find("album-sort").unwrap().name, "ALBUMSORT");
        assert_eq!(find("TSO2").unwrap().name, "ALBUMARTISTSORT");
        assert_eq!(find("grouping").unwrap().name, "CONTENTGROUP");
        assert!(find("RELEASETYPE").is_none(), "a custom field is not in the table");
        // A frame id is not a name: a custom frame described "TSO2" is found only by `find`.
        assert!(find_by_name("TSO2").is_none());
    }

    #[test]
    fn mp4_atoms_are_four_bytes() {
        for fd in FIELDS {
            if let Some(a) = fd.mp4 {
                assert!(atom_bytes(a).is_some(), "{} has a malformed atom {a:?}", fd.name);
            }
        }
        assert_eq!(atom_bytes("\u{a9}grp"), Some([0xA9, b'g', b'r', b'p']));
    }

    #[test]
    fn no_name_belongs_to_two_fields() {
        let mut seen: Vec<(String, &str)> = Vec::new();
        for fd in FIELDS {
            for n in std::iter::once(fd.name).chain(fd.aliases.iter().copied()) {
                if let Some((_, other)) = seen.iter().find(|(s, _)| eq(s, n)) {
                    panic!("{n} is both {other} and {}", fd.name);
                }
                seen.push((n.to_owned(), fd.name));
            }
        }
    }
}
