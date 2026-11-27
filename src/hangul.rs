//! Hangul composition for the todo input — folds the jamo stream the 2-Set
//! Korean keyboard emits into composed syllables, with no third-party crates.
//!
//! The GUI reads raw `-[NSEvent characters]` and is not an `NSTextInputClient`,
//! so the macOS text system never composes Hangul for it: a keydown on the
//! 2-Set Korean layout delivers one jamo per key (choseong forms for
//! consonant keys, jungseong forms for vowel keys), not syllables. [`compose`]
//! applies the standard KS X 5002 keyboard composition to such a stream, so
//! typing r-k-s-k — which delivers ㄱ ㅏ ㄴ ㅏ — ends up as 가나, exactly like
//! a text field would show.
//!
//! # Rules
//!
//! A pending syllable accumulates `(choseong, jungseong, jongseong)` and is
//! committed when the next character cannot attach to it:
//!
//! * a vowel fills an empty jungseong slot and conjoins with it when the
//!   pair forms a compound vowel (ㅗ + ㅏ → ㅘ, …);
//! * a consonant fills an empty jongseong slot (mapped to its jongseong
//!   form) and conjoins into a compound final (ㅂ + ㅅ → ㅄ, …);
//! * a consonant over an empty jungseong slot doubles the choseong
//!   (ㄱ + ㄱ → ㄲ) or commits and opens the next syllable;
//! * a vowel over a filled jongseong slot *detaches* it: the syllable
//!   commits without the final (with its first half, when the final is a
//!   compound) and the detached consonant opens the next syllable —
//!   ㄱ ㅏ ㄴ ㅏ commits 가 and continues with ㄴ + ㅏ = 나 (KS X 5002's
//!   "받침 분리" rule, which is what makes 바다/사과/입니다 type correctly);
//! * anything that is not a jamo (Latin letters, digits, punctuation,
//!   whitespace, precomposed syllables are decomposed to their slots
//!   first) commits the pending syllable and passes through untouched.
//!
//! # Syllable math
//!
//! Composing and decomposing precomposed syllables uses the algorithmic
//! Hangul block layout (Unicode §3.12): `S = AC00 + L·588 + V·28 + T`, with
//! `T = 0` meaning "no final" and final `t` mapping to the jongseong block
//! as `U+11A8 + t - 1`.
//!
//! Jamo that reach this app as compatibility forms (U+3131.., e.g. from some
//! pasteboard content) are mapped to their standard equivalents first, so
//! composition and rendering see one uniform alphabet.

/// Compose a jamo stream into syllables. Idempotent: a string that is
/// already composed passes through unchanged, which matters because the
/// todo input runs through [`compose`] after every frame's keystrokes.
///
/// ```
/// // ㄱ ㅏ ㄴ ㅏ (four keydowns) -> 가나
/// assert_eq!(hangul::compose("\u{1100}\u{1161}\u{11AB}\u{1161}"), "\u{AC00}\u{B098}");
/// ```
pub fn compose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending = Pending::default();
    for ch in text.chars() {
        match classify(ch) {
            Jamo::Cho(c) => pending.chooseong(c, &mut out),
            Jamo::Jung(v) => pending.jungseong(v, &mut out),
            Jamo::Jong(j) => pending.jongseong(j, &mut out),
            Jamo::Syllable { l, v, t } => {
                // A precomposed syllable (from an earlier frame's commit,
                // or from pasteboard text) seeds the pending state, so the
                // jamo after it still compose onto it: 가 + ㄴ -> 간.
                pending.flush(&mut out);
                pending.l = Some(l);
                pending.v = Some(v);
                pending.t = t;
            }
            Jamo::Other(ch) => {
                pending.flush(&mut out);
                out.push(ch);
            }
        }
    }
    pending.flush(&mut out);
    out
}

// ---------------------------------------------------------------------------
// Unicode block layout (Unicode Standard §3.12).
// ---------------------------------------------------------------------------

/// First precomposed syllable 가.
const SYL_BASE: u32 = 0xAC00;
/// Choseong (leading consonant) block: ㄱ..ㅎ, 19 letters.
const CHO_FIRST: u32 = 0x1100;
const CHO_LAST: u32 = 0x1112;
/// Jungseong (vowel) block: ㅏ..ㅣ, 21 letters.
const JUNG_FIRST: u32 = 0x1161;
const JUNG_LAST: u32 = 0x1175;
/// Jongseong (trailing consonant) block: ㄱ..ㅎ, 27 letters.
const JONG_FIRST: u32 = 0x11A8;
const JONG_LAST: u32 = 0x11C2;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
/// Compatibility jamo vowels ㅏ..ㅣ (the consonants have their own table —
/// most of them are compound finals, which map to jongseong forms).
const COMPAT_JUNG_FIRST: u32 = 0x314F;
const COMPAT_JUNG_LAST: u32 = 0x3163;

/// Compatibility vowel -> jungseong offset (`U+314F - offset = U+1161`).
const COMPAT_JUNG_OFFSET: u32 = 0x1FEE;

/// Choseong -> jongseong form of the same letter. ㄸ ㅃ ㅉ have no jongseong
/// form (they can never end a syllable) and are absent.
const CHO_TO_JONG: [(u32, u32); 16] = [
    (0x1100, 0x11A8), // ㄱ
    (0x1101, 0x11A9), // ㄲ
    (0x1102, 0x11AB), // ㄴ
    (0x1103, 0x11AE), // ㄷ
    (0x1105, 0x11AF), // ㄹ
    (0x1106, 0x11B7), // ㅁ
    (0x1107, 0x11B8), // ㅂ
    (0x1109, 0x11BA), // ㅅ
    (0x110A, 0x11BB), // ㅆ
    (0x110B, 0x11BC), // ㅇ
    (0x110C, 0x11BD), // ㅈ
    (0x110E, 0x11BE), // ㅊ
    (0x110F, 0x11BF), // ㅋ
    (0x1110, 0x11C0), // ㅌ
    (0x1111, 0x11C1), // ㅍ
    (0x1112, 0x11C2), // ㅎ
];

/// Compound vowels: `(first, second) -> compound` (KS X 5002).
const JUNG_COMPOUND: [(u32, u32, u32); 11] = [
    (0x1161, 0x1175, 0x1162), // ㅏ + ㅣ -> ㅐ
    (0x1163, 0x1175, 0x1164), // ㅑ + ㅣ -> ㅒ
    (0x1165, 0x1175, 0x1166), // ㅓ + ㅣ -> ㅔ
    (0x1167, 0x1175, 0x1168), // ㅕ + ㅣ -> ㅖ
    (0x1169, 0x1161, 0x116A), // ㅗ + ㅏ -> ㅘ
    (0x1169, 0x1162, 0x116B), // ㅗ + ㅐ -> ㅙ
    (0x1169, 0x1175, 0x116C), // ㅗ + ㅣ -> ㅚ
    (0x116E, 0x1165, 0x116F), // ㅜ + ㅓ -> ㅝ
    (0x116E, 0x1166, 0x1170), // ㅜ + ㅔ -> ㅞ
    (0x116E, 0x1175, 0x1171), // ㅜ + ㅣ -> ㅟ
    (0x1173, 0x1175, 0x1174), // ㅡ + ㅣ -> ㅢ
];

/// Compound finals: `(first, second) -> compound` (KS X 5002).
const JONG_COMPOUND: [(u32, u32, u32); 12] = [
    (0x11A8, 0x11BA, 0x11AA), // ㄱ + ㅅ -> ㄳ
    (0x11AB, 0x11BD, 0x11AC), // ㄴ + ㅈ -> ㄵ
    (0x11AB, 0x11C2, 0x11AD), // ㄴ + ㅎ -> ㄶ
    (0x11AF, 0x11A8, 0x11B0), // ㄹ + ㄱ -> ㄺ
    (0x11AF, 0x11B7, 0x11B1), // ㄹ + ㅁ -> ㄻ
    (0x11AF, 0x11B8, 0x11B2), // ㄹ + ㅂ -> ㄼ
    (0x11AF, 0x11BA, 0x11B3), // ㄹ + ㅅ -> ㄽ
    (0x11AF, 0x11C0, 0x11B4), // ㄹ + ㅌ -> ㄾ
    (0x11AF, 0x11C1, 0x11B5), // ㄹ + ㅍ -> ㄿ
    (0x11AF, 0x11C2, 0x11B6), // ㄹ + ㅎ -> ㅀ
    (0x11B8, 0x11BA, 0x11B9), // ㅂ + ㅅ -> ㅄ
    (0x11BA, 0x11BA, 0x11BB), // ㅅ + ㅅ -> ㅆ
];

/// Double choseong: `(first, second) -> compound` (ㄲ ㄸ ㅃ ㅆ ㅉ).
const CHO_DOUBLE: [(u32, u32, u32); 5] = [
    (0x1100, 0x1100, 0x1101), // ㄱ + ㄱ -> ㄲ
    (0x1103, 0x1103, 0x1104), // ㄷ + ㄷ -> ㄸ
    (0x1107, 0x1107, 0x1108), // ㅂ + ㅂ -> ㅃ
    (0x1109, 0x1109, 0x110A), // ㅅ + ㅅ -> ㅆ
    (0x110C, 0x110C, 0x110D), // ㅈ + ㅈ -> ㅉ
];

/// Compatibility consonant jamo -> the standard form of the same letter:
/// a choseong form for the 19 basic consonants, a jongseong form for the
/// 11 compound finals (ㄳ ㄵ ㄶ ㄺ ㄻ ㄼ ㄽ ㄾ ㄿ ㅀ ㅄ), which exist only
/// in the jongseong block.
const COMPAT_CONSONANT: [(u32, u32); 30] = [
    (0x3131, 0x1100), // ㄱ
    (0x3132, 0x1101), // ㄲ
    (0x3133, 0x11AA), // ㄳ
    (0x3134, 0x1102), // ㄴ
    (0x3135, 0x11AC), // ㄵ
    (0x3136, 0x11AD), // ㄶ
    (0x3137, 0x1103), // ㄷ
    (0x3138, 0x1104), // ㄸ
    (0x3139, 0x1105), // ㄹ
    (0x313A, 0x11B0), // ㄺ
    (0x313B, 0x11B1), // ㄻ
    (0x313C, 0x11B2), // ㄼ
    (0x313D, 0x11B3), // ㄽ
    (0x313E, 0x11B4), // ㄾ
    (0x313F, 0x11B5), // ㄿ
    (0x3140, 0x11B6), // ㅀ
    (0x3141, 0x1106), // ㅁ
    (0x3142, 0x1107), // ㅂ
    (0x3143, 0x1108), // ㅃ
    (0x3144, 0x11B9), // ㅄ
    (0x3145, 0x1109), // ㅅ
    (0x3146, 0x110A), // ㅆ
    (0x3147, 0x110B), // ㅇ
    (0x3148, 0x110C), // ㅈ
    (0x3149, 0x110D), // ㅉ
    (0x314A, 0x110E), // ㅊ
    (0x314B, 0x110F), // ㅋ
    (0x314C, 0x1110), // ㅌ
    (0x314D, 0x1111), // ㅍ
    (0x314E, 0x1112), // ㅎ
];

fn cho_to_jong(c: u32) -> Option<u32> {
    CHO_TO_JONG.iter().find(|(a, _)| *a == c).map(|(_, j)| *j)
}

fn cho_of_jong(j: u32) -> Option<u32> {
    CHO_TO_JONG.iter().find(|(_, b)| *b == j).map(|(a, _)| *a)
}

fn jung_compound(v1: u32, v2: u32) -> Option<u32> {
    JUNG_COMPOUND
        .iter()
        .find(|(a, b, _)| *a == v1 && *b == v2)
        .map(|(_, _, c)| *c)
}

fn jong_compound(t1: u32, t2: u32) -> Option<u32> {
    JONG_COMPOUND
        .iter()
        .find(|(a, b, _)| *a == t1 && *b == t2)
        .map(|(_, _, c)| *c)
}

/// First and second half of a compound final (`ㅄ -> (ㅂ, ㅅ)`).
fn jong_compound_parts(t: u32) -> Option<(u32, u32)> {
    JONG_COMPOUND
        .iter()
        .find(|(_, _, c)| *c == t)
        .map(|(a, b, _)| (*a, *b))
}

fn cho_double(c1: u32, c2: u32) -> Option<u32> {
    CHO_DOUBLE
        .iter()
        .find(|(a, b, _)| *a == c1 && *b == c2)
        .map(|(_, _, c)| *c)
}

/// Compose one syllable from its slots (`ㅎ ㅏ ㄴ -> 한`). The final's
/// block position is its `T` index plus one — `T = 0` means "no final".
fn syllable(l: u32, v: u32, t: Option<u32>) -> char {
    let t = t.map_or(0, |j| j - JONG_FIRST + 1);
    char::from_u32(SYL_BASE + (l - CHO_FIRST) * V_COUNT * T_COUNT + (v - JUNG_FIRST) * T_COUNT + t)
        .expect("jamo slots always compose to a valid syllable")
}

// ---------------------------------------------------------------------------
// Pending-syllable state machine.
// ---------------------------------------------------------------------------

/// One jamo as seen by the state machine.
enum Jamo {
    /// Consonant key output, in choseong form.
    Cho(u32),
    /// Vowel key output (jungseong form; compatibility forms pre-mapped).
    Jung(u32),
    /// Jongseong-form jamo — never typed (the layout emits choseong forms
    /// for consonants), but arrives in pasted fully-decomposed text.
    Jong(u32),
    /// Precomposed syllable, decomposed to its slots.
    Syllable { l: u32, v: u32, t: Option<u32> },
    /// Anything else — passes through and resets the pending syllable.
    Other(char),
}

fn classify(ch: char) -> Jamo {
    let cp = ch as u32;
    if (CHO_FIRST..=CHO_LAST).contains(&cp) {
        return Jamo::Cho(cp);
    }
    if (JUNG_FIRST..=JUNG_LAST).contains(&cp) {
        return Jamo::Jung(cp);
    }
    if (JONG_FIRST..=JONG_LAST).contains(&cp) {
        return Jamo::Jong(cp);
    }
    if (COMPAT_JUNG_FIRST..=COMPAT_JUNG_LAST).contains(&cp) {
        return Jamo::Jung(cp - COMPAT_JUNG_OFFSET);
    }
    if let Some(&std) = COMPAT_CONSONANT
        .iter()
        .find(|(compat, _)| *compat == cp)
        .map(|(_, std)| std)
    {
        return if (CHO_FIRST..=CHO_LAST).contains(&std) {
            Jamo::Cho(std)
        } else {
            Jamo::Jong(std)
        };
    }
    if (SYL_BASE..SYL_BASE + 11172).contains(&cp) {
        let i = cp - SYL_BASE;
        let l = CHO_FIRST + i / (V_COUNT * T_COUNT);
        let v = JUNG_FIRST + (i % (V_COUNT * T_COUNT)) / T_COUNT;
        let t = i % T_COUNT;
        return Jamo::Syllable {
            l,
            v,
            t: (t != 0).then(|| JONG_FIRST + t - 1),
        };
    }
    Jamo::Other(ch)
}

/// The syllable being composed, one (leading consonant, vowel, trailing
/// consonant) triple. Committed lazily, when the next jamo cannot attach.
#[derive(Default)]
struct Pending {
    l: Option<u32>,
    v: Option<u32>,
    /// Jongseong-form codepoint, simple or already-compounded.
    t: Option<u32>,
}

impl Pending {
    /// Commit to `out`: a complete syllable when both a consonant and a
    /// vowel are present, otherwise the lone jamo (letters typed before
    /// their syllable forms, which is also what a text field shows).
    fn flush_into(self, out: &mut String) {
        let (l, v, t) = (self.l, self.v, self.t);
        match (l, v) {
            (Some(l), Some(v)) => out.push(syllable(l, v, t)),
            (Some(l), None) => out.push(char::from_u32(l).expect("choseong codepoint")),
            (None, Some(v)) => out.push(char::from_u32(v).expect("jungseong codepoint")),
            (None, None) => {}
        }
    }

    fn flush(&mut self, out: &mut String) {
        let empty = Pending::default();
        let drained = std::mem::replace(self, empty);
        drained.flush_into(out);
    }

    /// Consonant keydown (choseong form).
    fn chooseong(&mut self, c: u32, out: &mut String) {
        match (self.l, self.v, self.t) {
            // Opens a new syllable.
            (None, None, _) => self.l = Some(c),
            // Consonant over a lone vowel: the vowel commits as its own
            // letter and the consonant opens the next syllable.
            (None, Some(_), _) => {
                self.flush(out);
                self.l = Some(c);
            }
            // Second consonant before any vowel: doubles the first
            // (ㄱ + ㄱ -> ㄲ) or commits and opens the next syllable.
            (Some(l), None, _) => match cho_double(l, c) {
                Some(d) => self.l = Some(d),
                None => {
                    self.flush(out);
                    self.l = Some(c);
                }
            },
            // Consonant over a filled vowel, empty final: fills the final
            // with the letter's jongseong form — unless the consonant is
            // ㄸ/ㅃ/ㅉ, which cannot end a syllable.
            (Some(_), Some(_), None) => match cho_to_jong(c) {
                Some(j) => self.t = Some(j),
                None => {
                    self.flush(out);
                    self.l = Some(c);
                }
            },
            // Consonant over a filled final: extends a compound final
            // (받침 ㅂ + ㅅ -> ㅄ) or commits and opens the next syllable.
            (Some(_), Some(_), Some(t)) => {
                let compound = cho_to_jong(c).and_then(|j| jong_compound(t, j));
                match compound {
                    Some(d) => self.t = Some(d),
                    None => {
                        self.flush(out);
                        self.l = Some(c);
                    }
                }
            }
        }
    }

    /// Vowel keydown (jungseong form).
    fn jungseong(&mut self, v: u32, out: &mut String) {
        match (self.l, self.v, self.t) {
            // Fills an empty vowel slot — as a lone vowel when no
            // consonant is pending (still renderable, and it can still
            // grow a consonant and a vowel after it).
            (None, None, _) | (Some(_), None, _) => self.v = Some(v),
            // Vowel over a filled vowel slot, empty final: conjoins into a
            // compound vowel (ㅗ + ㅏ -> ㅘ) or commits and stands alone.
            (_, Some(v2), None) => match jung_compound(v2, v) {
                Some(d) => self.v = Some(d),
                None => {
                    self.flush(out);
                    self.v = Some(v);
                }
            },
            // Vowel over a filled final — the 받침 분리 rule: the final
            // detaches and opens the next syllable (its second half, when
            // it is a compound: 값 + ㅏ -> 갑 + 사). This is what makes
            // 가나/바다/사과 type correctly without manual commits.
            // (A final without a leading consonant is unreachable — finals
            // are only ever filled after a consonant and a vowel.)
            (_, Some(_), Some(_)) => {
                let t = self.t.take().expect("checked");
                let carry = match jong_compound_parts(t) {
                    Some((t1, t2)) => {
                        self.t = Some(t1);
                        self.flush(out);
                        t2
                    }
                    None => {
                        self.flush(out);
                        t
                    }
                };
                match cho_of_jong(carry) {
                    Some(c) => self.l = Some(c),
                    None => out.push(char::from_u32(carry).expect("jongseong codepoint")),
                }
                self.v = Some(v);
            }
        }
    }

    /// Jongseong-form jamo — arrives from pasted fully-decomposed text.
    fn jongseong(&mut self, j: u32, out: &mut String) {
        match (self.l, self.v, self.t) {
            (Some(_), Some(_), None) => self.t = Some(j),
            (Some(_), Some(_), Some(t)) => match jong_compound(t, j) {
                Some(d) => self.t = Some(d),
                None => {
                    self.flush(out);
                    out.push(char::from_u32(j).expect("jongseong codepoint"));
                }
            },
            _ => {
                self.flush(out);
                out.push(char::from_u32(j).expect("jongseong codepoint"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests. Expected values verified against the Unicode database (syllable
// indices) and the KS X 5002 composition rules; jamo inputs are spelled as
// `\u` escapes because source-level Hangul jamo literals are ambiguous
// between the standard and compatibility blocks.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::compose;

    /// `(input, expected)` — jamo streams and what a text field shows.
    const CASES: [(&str, &str, &str); 32] = [
        (" lone cho stays", "\u{1100}", "\u{1100}"),
        (" lone jung stays", "\u{1161}", "\u{1161}"),
        ("L+V", "\u{1100}\u{1161}", "\u{AC00}"),
        ("L+V+T", "\u{1100}\u{1161}\u{11AB}", "\u{AC04}"),
        ("hangul", "\u{1112}\u{1161}\u{11AB}\u{1100}\u{1173}\u{1105}", "\u{D55C}\u{AE00}"),
        ("compound T", "\u{1100}\u{1161}\u{1107}\u{1109}", "\u{AC12}"),
        ("compound T + vowel", "\u{1100}\u{1161}\u{1107}\u{1109}\u{110B}\u{1175}", "\u{AC12}\u{C774}"),
        ("compound T detach", "\u{1100}\u{1161}\u{1107}\u{1109}\u{1161}", "\u{AC11}\u{C0AC}"),
        ("simple T detach", "\u{1100}\u{1161}\u{11AB}\u{1161}", "\u{AC00}\u{B098}"),
        ("T detach gamma", "\u{1100}\u{1161}\u{1106}\u{1161}", "\u{AC00}\u{B9C8}"),
        ("jong compound", "\u{1100}\u{1161}\u{1100}\u{1109}", "\u{AC03}"),
        ("cho double", "\u{1100}\u{1100}\u{1161}", "\u{AE4C}"),
        ("ssang", "\u{1109}\u{1109}", "\u{110A}"),
        ("lone vowel + cons", "\u{1161}\u{1100}\u{1161}", "\u{1161}\u{AC00}"),
        ("vowel compound", "\u{1169}\u{1161}", "\u{116A}"),
        ("mweos", "\u{1106}\u{116E}\u{1165}\u{1109}", "\u{BB63}"),
        ("mueos", "\u{1106}\u{116E}\u{110B}\u{1165}\u{1109}", "\u{BB34}\u{C5C7}"),
        ("sagwa", "\u{1109}\u{1161}\u{1100}\u{1169}\u{1161}", "\u{C0AC}\u{ACFC}"),
        ("jamo after syllable", "\u{AC00}\u{11AB}", "\u{AC04}"),
        ("syllable after jamo", "\u{1100}\u{1161}\u{AC00}", "\u{AC00}\u{AC00}"),
        ("jamo no double", "\u{1100}\u{1102}", "\u{1100}\u{1102}"),
        ("no jong form cho", "\u{1104}\u{1161}", "\u{B530}"),
        ("latin + korean", "ab\u{1100}\u{1161}", "ab\u{AC00}"),
        ("punct resets", "\u{1100}\u{1161}!\u{1102}\u{1175}", "\u{AC00}!\u{B2C8}"),
        ("space resets", "a \u{1100}\u{1161}", "a \u{AC00}"),
        ("compat jamo", "\u{3142}\u{314F}", "\u{BC14}"),
        ("compat vowel compound", "\u{3157}\u{314F}", "\u{116A}"),
        ("compat jong after syl", "\u{AC11}\u{3144}", "\u{AC11}\u{11B9}"),
        ("jong direct w/ V", "\u{1100}\u{1161}\u{11BA}", "\u{AC13}"),
        ("jong compound direct", "\u{1100}\u{1161}\u{11B8}\u{11BA}", "\u{AC12}"),
        ("idempotent values", "\u{AC12}", "\u{AC12}"),
        ("idempotent hangul", "\u{D55C}\u{AE00}!", "\u{D55C}\u{AE00}!"),
    ];

    #[test]
    fn composition_cases() {
        for (name, input, expected) in CASES {
            assert_eq!(compose(input), expected, "case: {name}");
        }
    }

    /// Real words typed on the 2-Set layout, spelled with precomposed
    /// syllables on both sides (those are unambiguous literals).
    #[test]
    fn words_pass_through_and_compose() {
        // Precomposed words are idempotent.
        for word in ["한글", "값", "입니다", "종료", "사과", "무엇", "밟다"] {
            assert_eq!(compose(word), word, "word: {word}");
        }
        // The same words as raw jamo streams compose to them. Inputs are
        // built from codepoints to keep the blocks unambiguous.
        let jamo = |cps: &[u32]| -> String {
            cps.iter().map(|&c| char::from_u32(c).unwrap()).collect()
        };
        assert_eq!(compose(&jamo(&[0x1112, 0x1161, 0x11AB, 0x1100, 0x1173, 0x1105])), "한글");
        assert_eq!(compose(&jamo(&[0x1100, 0x1161, 0x1107, 0x1109])), "값");
        assert_eq!(
            compose(&jamo(&[0x110B, 0x1175, 0x1107, 0x1102, 0x1175, 0x1103, 0x1161])),
            "입니다"
        );
        assert_eq!(
            compose(&jamo(&[0x1109, 0x1161, 0x1100, 0x1169, 0x1161])),
            "사과"
        );
    }

    /// Mixed Latin/Korean text (a realistic todo entry).
    #[test]
    fn mixed_text() {
        let input = "buy \u{1100}\u{1161}\u{1102}\u{1175} milk";
        assert_eq!(compose(input), "buy \u{AC00}\u{B2C8} milk");
    }
}
