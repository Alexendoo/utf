/// Generates a variation of the suffix array for fast codepoint name searching
/// by substring.
///
/// https://en.wikipedia.org/wiki/Suffix_array (note: Wikipedia uses 1-based
/// indexing but here we will use 0-based for implementation parity)
///
/// Suffix arrays provide fast substring lookup into a string by allowing one
/// to binary search for any suffix in said string, however for our purposes
/// we need to search multiple strings (each Unicode character name), matching
/// all codepoints whose names contain said substring. To achieve this we
/// concatenate all the names together, storing the codepoint alongside the
/// indexes.
///
/// For example the imaginary codepoints "one" as \x01 and "five" as \x05 would
/// be joined to form S = "one$five$". '$' is a sentinel character that is
/// lexicographically smaller than all other characters, in the implementation
/// a newline character is used to make the generated file look nicer.
///
/// Because no pattern contains the sentinel, the suffix "ne$five$" is
/// equivalent to "ne$", we also omit suffixes that begin with a sentinel, e.g.
/// "$five$" (i = 3) or "$" (i = 8) to save some bytes.
///
/// Starting with the suffixes of S
///
/// | Suffix | i | c |
/// | ------ | - | - |
/// | one$   | 0 | 1 |
/// | ne$    | 1 | 1 |
/// | e$     | 2 | 1 |
/// | five$  | 4 | 5 |
/// | ive$   | 5 | 5 |
/// | ve$    | 6 | 5 |
/// | e$     | 7 | 5 |
///
/// They are then sorted lexicographically by suffix to become
///
/// | Suffix | i | c |
/// | ------ | - | - |
/// | e$     | 2 | 1 |
/// | e$     | 7 | 5 |
/// | five$  | 4 | 5 |
/// | ive$   | 5 | 5 |
/// | ne$    | 1 | 1 |
/// | one$   | 0 | 1 |
/// | ve$    | 6 | 5 |
///
/// Since we don't need to know where in the combined string our suffix is, only
/// which codepoint is corresponds to we can join the entries for "e$", losing
/// the information that there is a suffix at index 7. This costs a few bytes
/// per entry since the codepoint array is now an array of arrays, however since
/// there are around 18x fewer entries in total for this data set the trade-off
/// is well worth it.
///
/// After removing duplicates and the suffixes the remaining data is:
///
/// - The combined string S = "one$five$",
/// - The suffix array A,
/// - The corresponding codepoints C
///
/// | i | A[i] | C[i] |
/// | - | ---- | ---- |
/// | 0 |    2 | 1, 5 |
/// | 1 |    4 |    5 |
/// | 2 |    5 |    5 |
/// | 3 |    1 |    1 |
/// | 4 |    0 |    1 |
/// | 5 |    6 |    5 |
///
/// This is the final table that we will use to search for name matches. The
/// suffix for each entry at i can be enumerated by iterating through the string
/// S beginning at index A[i].
///
/// | i | S[A[i]..] | A[i] | C[i] |
/// | - | -------   | ---- | ---- |
/// | 0 | e$five$   |    2 | 1, 5 |
/// | 1 | five$     |    4 |    5 |
/// | 2 | ive$      |    5 |    5 |
/// | 3 | ne$five$  |    1 |    1 |
/// | 4 | one$five$ |    0 |    1 |
/// | 5 | ve$       |    6 |    5 |
///
/// To find all the matches we have to find all the suffixes that begin with the
/// input pattern. Since the table is sorted these suffixes are all contiguous,
/// to locate them we perform two binary searches. One search to find the first
/// matching suffix and for the last.
///
/// https://en.wikipedia.org/wiki/Suffix_array#Applications
///
/// With the range of suffixes found the list of codepoints can be enumerated
/// for the interval C[first..last].
///
/// # Extra
///
/// Not shown is a further optimisation where each Unicode character name is
/// split into individual words, e.g. "DIGIT ONE" adds "DIGIT$" and "ONE$" to
/// the table separately. This allows us to deduplicate the many occurrences of
/// words such as "LETTER" that appear in the middle of names. This reduction
/// makes both the tables A and C smaller as well as the combined string S,
/// drastically reducing the total size requirement.
///
/// Additionally a small extra saving can be had by removing duplicate suffixes
/// from the same codepoint, for example in "LEFT SQUARE BRACKET" when split
/// into words "LEFT$", "SQUARE$" and "BRACKET$" the suffix "T$" appears twice.
/// Only one entry is needed to discover the codepoint contains the suffix "T$",
/// so the other is removed.
///
/// This means that multiword searches will no longer work directly since no
/// suffixes contain spaces. Instead we also split the input pattern into words
/// and search individually once for each word. Each word gives us a set of
/// codepoints, the final result in the union of all word subresults.
///
/// For example "LATIN LETTER" is split into patterns "LATIN" and "LETTER",
/// the final result is codepoints that match both "LATIN" and "LETTER". This
/// means that word order of the input pattern no longer has an effect on the
/// result, allowing "LATIN LETTER" to match "LATIN SMALL LETTER A".

extern crate ucd_raw;

use std::collections::HashMap;
use std::fs::create_dir_all;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::time::Instant;
use ucd_raw::Character;
use ucd_raw::CHARACTERS;

#[derive(Debug)]
struct Entry {
    index: usize,
    codepoints: Vec<u32>,
}

#[derive(Debug)]
struct TempSuffix<'a> {
    suffix: &'a str,
    entry: Entry,
}

fn char_iter() -> impl Iterator<Item = &'static Character> {
    CHARACTERS
        .iter()
        .filter(|character| match character.codepoint {
            0x03400...0x04DB5 => false,
            0x04E00...0x09FEA => false,
            0x20000...0x2A6D6 => false,
            0x2A700...0x2B734 => false,
            0x2B740...0x2B81D => false,
            0x2B820...0x2CEA1 => false,
            0x2CEB0...0x2EBE0 => false,
            0x17000...0x187EC => false,
            0x1B170...0x1B2FB => false,
            0x0F900...0x0FA6D => false,
            0x0FA70...0x0FAD9 => false,
            0x2F800...0x2FA1D => false,
            _ => true,
        })
}

fn suffixes<'a>(string: &'a str) -> impl Iterator<Item = (usize, &'a str)> {
    (0..string.len()).map(move |i| &string[i..]).enumerate()
}

fn main() {
    println!("indexing {} codepoints...", char_iter().count());

    let start = Instant::now();

    let mut combined = String::new();
    let mut visited_words = HashMap::new();
    let mut temp_suffixes = Vec::new();

    create_dir_all("src/generated").unwrap();
    let out = File::create("src/generated/generated.rs").unwrap();
    let mut out = BufWriter::new(out);

    for character in char_iter() {
        for word in character.name.split_whitespace() {
            let mut start;

            if visited_words.contains_key(word) {
                start = visited_words[word];
            } else {
                start = combined.len();

                visited_words.insert(word, start);
                combined.push_str(word);
                combined.push('\n');
            }

            for (offset, suffix) in suffixes(word) {
                temp_suffixes.push(TempSuffix {
                    suffix,
                    entry: Entry {
                        index: start + offset,
                        codepoints: vec![character.codepoint],
                    },
                });
            }
        }
    }

    temp_suffixes.sort_unstable_by_key(|temp_suffix| temp_suffix.suffix);

    println!("length before dedup: {}", temp_suffixes.len());

    temp_suffixes.dedup_by(|a, b| {
        let equal = a.suffix == b.suffix;

        if equal {
            b.entry.codepoints.extend_from_slice(&a.entry.codepoints);
        }

        equal
    });

    for suffix in &mut temp_suffixes {
        let codepoints = &mut suffix.entry.codepoints;

        codepoints.sort_unstable();
        codepoints.dedup();
    }

    println!("after: {}", temp_suffixes.len());

    writeln!(out, "// Auto-generated by `src/bin/gen`\n").unwrap();
    writeln!(out, "use table::Entry;").unwrap();

    writeln!(
        out,
        "pub const ENTRIES: &[Entry] = &{:#?};",
        temp_suffixes
            .into_iter()
            .map(|temp_suffix| temp_suffix.entry)
            .collect::<Vec<_>>()
    ).unwrap();

    writeln!(out, "pub const COMBINED: &[u8] = b\"{}\";", combined).unwrap();

    println!("Generated in {:?}", start.elapsed());
}
