//! Each language's rules on sentences checked against espeak-ng.

use crate::text::{Piece, split};
use crate::{Language, kokoro, phonemize};

fn es(text: &str) -> String {
    phonemize(Language::Spanish, text)
}

fn it(text: &str) -> String {
    phonemize(Language::Italian, text)
}

fn pt(text: &str) -> String {
    phonemize(Language::Portuguese, text)
}

fn fr(text: &str) -> String {
    phonemize(Language::French, text)
}

#[test]
fn a_kokoro_voice_names_its_language_by_its_first_letter() {
    assert_eq!(
        Language::for_kokoro_voice("ef_dora"),
        Some(Language::Spanish)
    );
    assert_eq!(
        Language::for_kokoro_voice("im_nicola"),
        Some(Language::Italian)
    );
    assert_eq!(
        Language::for_kokoro_voice("pf_dora"),
        Some(Language::Portuguese)
    );
    assert_eq!(
        Language::for_kokoro_voice("ff_siwis"),
        Some(Language::French)
    );
    assert_eq!(Language::for_kokoro_voice("af_heart"), None);
    assert_eq!(Language::for_kokoro_voice(""), None);
}

#[test]
fn text_splits_into_words_numbers_marks_and_dashes() {
    let pieces: Vec<Piece<'_>> = split("l'uomo, 3,5 -- 1.000!")
        .iter()
        .map(|t| t.piece)
        .collect();
    let want = [
        Piece::Word("l'uomo"),
        Piece::Mark(','),
        Piece::Number("3,5"),
        Piece::Break,
        Piece::Number("1.000"),
        Piece::Mark('!'),
    ];
    assert_eq!(pieces, want);
}

#[test]
fn spanish_reads_as_espeak_does() {
    let said = es("Hola, ¿cómo estás? Todo el lago, la chica y el perro.");
    assert_eq!(
        said,
        "ˈola, ¿kˈomo estˈas? tˈoðo el lˈaɣo, la t^ʃˈika i el pˈero."
    );
}

#[test]
fn spanish_sounds_change_with_the_word_beside_them() {
    assert_eq!(
        es("un beso, un gato en Madrid"),
        "ˈum bˈeso, ˈuŋ ɡˈato en maðɾˈid"
    );
    assert_eq!(
        es("Madrid usted, la ciudad."),
        "maðɾˈið ustˈed, la θjuðˈad."
    );
    assert_eq!(es("Teodoro--dijo la Nela."), "tˌeoðˈoɾo dˈixo la nˈela.");
}

#[test]
fn spanish_vowels_join_or_stay_apart_as_spelled() {
    let said = es("sabéis, prohibir, rueda, cliente");
    assert_eq!(said, "saβˈeis, pɾˌoiβˈiɾ, ruˈeða, kliˈɛnte");
    let said = es("independencia, rápidamente");
    assert_eq!(said, "ˌindependˈɛnθja, rˈapiðamˈente");
}

#[test]
fn spanish_numbers_are_said_in_words() {
    let said = es("el año 1999, a las 3,5");
    let want = "el ˈaɲo mˈil nˌoβeθjˈɛntos noβˈɛnta i nwˈeβe, a las tɾˈes kˈoma θˈinko";
    assert_eq!(said, want);
    assert_eq!(es("21"), "bˌe^ɪntiˈuno");
}

#[test]
fn italian_reads_as_espeak_does() {
    assert_eq!(it("Il gatto e la pizza."), "il ɡˈatːo e la pˈit^sːa.");
    let said = it("Ciao, come stai? Il tavolo e la macchina.");
    assert_eq!(said, "t^ʃˈao, kˌome stˈaj? il tˈavolo e la mˈakːina.");
    let said = it("gli gnocchi, il figlio, la scienza");
    assert_eq!(said, "ʎɪ ɲˈokːɪ, il fˈiʎo, la ʃˈɛnt^sa");
}

#[test]
fn italian_joins_a_word_cut_short_to_the_next() {
    assert_eq!(it("l'uomo dell'acqua"), "lwˈɔmo dellˈakːwa");
    assert_eq!(it("perché, città, così"), "perkˈe, t^ʃitːˈa, kozˈi");
}

#[test]
fn italian_numbers_are_said_a_group_at_a_time() {
    assert_eq!(it("2026"), "dˈue mˈila ventisˈɛi");
    assert_eq!(it("3,5"), "trˈe vˈirɡola t^ʃˈinkwe");
}

#[test]
fn portuguese_reads_as_espeak_does() {
    let said = pt("O menino disse que não tinha visto a cidade.");
    let want = "ʊ mˌenˈinʊ d^ʒˈisy ky nˌɐ̃^ʊ̃ t^ʃˌiɲæ vˈistw a sˌidˈad^ʒy.";
    assert_eq!(said, want);
    let said = pt("um coração alto e volta");
    let want = "u\u{303}ŋ kˌoɾasˈɐ\u{303}^ʊ\u{303} ˈa^ʊtw i vˈɔltæ";
    assert_eq!(said, want);
}

#[test]
fn portuguese_links_a_word_to_the_vowel_after_it() {
    let said = pt("Levantou-se e foi embora.");
    assert_eq!(said, "lˌevɐ̃ŋtˈowsj i fo^ɪ ˌe^ɪmbˈɔɾæ.");
    let said = pt("Os homens de hoje, o sol e a lua.");
    assert_eq!(said, "ʊs ˈome^ɪŋz d^ʒy ˈoʒy, ʊ sˈɔl i a lˈuæ.");
}

#[test]
fn portuguese_numbers_are_said_in_words() {
    let want = "mˈi^ʊ ˈo^ɪtʊsˈe^ɪŋtʊzˌi novˈe^ɪŋtæ";
    assert_eq!(pt("1890"), want);
}

#[test]
fn french_reads_as_espeak_does() {
    let said = fr("la campagne, le compagnon, la fille et la ville");
    assert_eq!(said, "la kɑ̃pˈaɲ, lə kɔ̃panjˈɔ̃, la fˈij e la vˈil");
    let said = fr("Catherine acheva maintenant.");
    assert_eq!(said, "katʁˈin aʃvˈa mɛ̃tnˈɑ̃.");
}

#[test]
fn french_links_a_silent_consonant_to_a_vowel() {
    let said = fr("Est-ce que vous venez? Dit-il.");
    assert_eq!(said, "ɛs kə vu vənˈe? dˈitˈil.");
    let said = fr("un grand homme, les héros");
    assert_eq!(said, "œ̃ ɡʁˈɑ̃t ˈɔm, le eʁˈo");
    let said = fr("Il y a vingt-quatre heures.");
    assert_eq!(said, "il i a vˈɛ̃tkˈatʁ ˈœʁ.");
}

#[test]
fn french_numbers_stress_only_their_last_word() {
    assert_eq!(fr("1987"), "mil nœfsɑ̃ katʁəvɛ̃sˈɛt");
    assert_eq!(fr("3,5"), "tʁwa viʁɡyl sˈɛ̃k");
}

#[test]
fn kokoro_spelling_folds_ties_into_single_letters() {
    let said = kokoro(Language::Spanish, "la chica, hay un rey");
    assert_eq!(said, "la ʧˈika, ˈI ˈun rˈA");
    assert!(!kokoro(Language::Italian, "la pizza").contains('^'));
}
