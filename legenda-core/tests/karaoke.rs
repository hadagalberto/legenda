//! Word-level transcript, page grouping and karaoke rendering tests.

use legenda_core::ass;
use legenda_core::karaoke::{estimate_text_width, render, KaraokeOptions, KaraokePreset};
use legenda_core::time::Time;
use legenda_core::transcript::{
    group_pages, parse_transcript, PageOptions, TranscriptFormat, Word,
};

const WHISPERX: &str = r#"{
  "text": "Hey, this is amazing. 🔥",
  "language": "en",
  "segments": [
    {
      "start": 0.0, "end": 1.6, "text": " Hey, this is",
      "words": [
        {"word": " Hey,", "start": 0.0, "end": 0.32, "score": 0.99},
        {"word": " this", "start": 0.34, "end": 0.70, "score": 0.98},
        {"word": " is", "score": 0.95}
      ]
    },
    {
      "start": 2.2, "end": 3.6, "text": " amazing. 🔥",
      "words": [
        {"word": " amazing.", "start": 2.2, "end": 2.9, "score": 0.97},
        {"word": " 🔥", "score": 0.55}
      ]
    }
  ]
}"#;

/// The maintainer's reproduction: 14 words, starts 380 ms apart.
const REPRO: &str = r#"{
  "segments": [{
    "start": 0.0, "end": 5.3,
    "words": [
      {"word": "Hoje", "start": 0.00, "end": 0.34},
      {"word": "eu", "start": 0.38, "end": 0.72},
      {"word": "vou", "start": 0.76, "end": 1.10},
      {"word": "te", "start": 1.14, "end": 1.48},
      {"word": "mostrar", "start": 1.52, "end": 1.86},
      {"word": "o", "start": 1.90, "end": 2.24},
      {"word": "segredo", "start": 2.28, "end": 2.62},
      {"word": "que", "start": 2.66, "end": 3.00},
      {"word": "ninguém", "start": 3.04, "end": 3.38},
      {"word": "conta", "start": 3.42, "end": 3.76},
      {"word": "sobre", "start": 3.80, "end": 4.14},
      {"word": "edição", "start": 4.18, "end": 4.52},
      {"word": "de", "start": 4.56, "end": 4.90},
      {"word": "vídeo", "start": 4.94, "end": 5.28}
    ]
  }]
}"#;

const WHISPER_CPP: &str = r#"{
  "text": "Olá mundo",
  "transcription": [
    {
      "text": " Olá mundo",
      "offsets": {"start": 1000, "end": 2500},
      "tokens": [
        {"text": " Olá", "offsets": {"start": 1000, "end": 1600}},
        {"text": " mundo", "offsets": {"start": 1650, "end": 2500}}
      ]
    }
  ]
}"#;

const OPENAI: &str = r#"{
  "text": "Hello world again",
  "duration": 3.0,
  "words": [
    {"word": "Hello", "start": 0.0, "end": 0.5},
    {"word": "world", "start": 0.6, "end": 1.2},
    {"word": "again", "start": 1.3, "end": 1.8}
  ],
  "segments": [{"start": 0.0, "end": 1.8, "text": "Hello world again"}]
}"#;

fn timed(text: &str, start_ms: i64, end_ms: i64) -> Word {
    Word {
        text: text.to_owned(),
        start: Time::from_millis(start_ms),
        end: Time::from_millis(end_ms),
        score: None,
    }
}

fn style_fields(ass_text: &str) -> Vec<&str> {
    ass_text
        .lines()
        .find(|line| line.starts_with("Style:"))
        .expect("ASS style line")
        .split(',')
        .collect()
}

#[test]
fn whisperx_known_vectors_and_interpolation() {
    let transcript = parse_transcript(WHISPERX).unwrap();
    assert_eq!(transcript.format, TranscriptFormat::WhisperX);
    assert_eq!(transcript.segments.len(), 2);
    assert_eq!(transcript.words.len(), 5);
    assert_eq!(transcript.words[0].text, "Hey,");
    assert_eq!(transcript.words[0].start, Time::ZERO);
    assert_eq!(transcript.words[0].end, Time::from_millis(320));
    assert_eq!(transcript.words[1].start, Time::from_millis(340));
    assert_eq!(transcript.words[1].end, Time::from_millis(700));
    assert_eq!(transcript.words[2].text, "is");
    assert_eq!(transcript.words[2].start, Time::from_millis(700));
    assert!(transcript.words[2].end > transcript.words[2].start);
    assert!(transcript.words[2].end <= transcript.segments[0].end);
    assert_eq!(transcript.words[3].text, "amazing.");
    assert_eq!(transcript.words[3].start, Time::from_millis(2_200));
    assert_eq!(transcript.words[4].start, Time::from_millis(2_900));
    assert!(transcript.words[4].end > transcript.words[4].start);
    assert!(transcript.words[4].end <= transcript.segments[1].end);
    for word in &transcript.words {
        assert!(word.end >= word.start, "reversed word timing: {word:?}");
    }
}

#[test]
fn whisper_cpp_full_json_known_vectors() {
    let transcript = parse_transcript(WHISPER_CPP).unwrap();
    assert_eq!(transcript.format, TranscriptFormat::WhisperCpp);
    assert_eq!(transcript.words.len(), 2);
    assert_eq!(transcript.words[0].text, "Olá");
    assert_eq!(transcript.words[0].start, Time::from_millis(1_000));
    assert_eq!(transcript.words[0].end, Time::from_millis(1_600));
    assert_eq!(transcript.words[1].text, "mundo");
    assert_eq!(transcript.words[1].start, Time::from_millis(1_650));
    assert_eq!(transcript.words[1].end, Time::from_millis(2_500));
}

#[test]
fn openai_verbose_json_known_vectors_and_missing_timings() {
    let transcript = parse_transcript(OPENAI).unwrap();
    assert_eq!(transcript.format, TranscriptFormat::OpenAiVerbose);
    assert_eq!(transcript.words.len(), 3);
    assert_eq!(transcript.words[2].text, "again");
    assert_eq!(transcript.words[2].start, Time::from_millis(1_300));

    let untimed = r#"{"words":[
        {"word":"a","start":0.0,"end":0.2},
        {"word":"bb"},
        {"word":"c","start":0.8,"end":1.0}
    ]}"#;
    let transcript = parse_transcript(untimed).unwrap();
    assert_eq!(transcript.words.len(), 3);
    assert_eq!(transcript.words[1].text, "bb");
    assert_eq!(transcript.words[1].start, Time::from_millis(200));
    assert_eq!(transcript.words[1].end, Time::from_millis(700));

    // Untimed words follow the previous word's end and are assigned to one
    // segment only, even when a segments array is present.
    let with_segments = r#"{
      "words": [
        {"word": "a", "start": 0.0, "end": 0.4},
        {"word": "b"},
        {"word": "c", "start": 1.2, "end": 1.6}
      ],
      "segments": [{"start": 0.0, "end": 0.8}, {"start": 1.0, "end": 2.0}]
    }"#;
    let transcript = parse_transcript(with_segments).unwrap();
    assert_eq!(transcript.segments.len(), 2);
    assert_eq!(transcript.words.len(), 3);
    assert_eq!(transcript.segments[0].words.len(), 2);
    assert_eq!(transcript.segments[1].words.len(), 1);
}

#[test]
fn known_invalid_transcripts_are_rejected() {
    assert!(parse_transcript("not json").is_err());
    assert!(parse_transcript("{}").is_err());
    assert!(parse_transcript(r#"{"segments":[{}]}"#).is_err());
    assert!(parse_transcript(r#"{"transcription":[{"text":"hi"}]}"#).is_err());
    assert!(parse_transcript(r#"{"words":[{"word":"x"}]}"#).is_err());
    assert!(
        parse_transcript(r#"{"segments":[{"start":0,"end":0,"words":[{"word":"x"}]}]}"#).is_err()
    );
}

#[test]
fn pages_group_by_pauses_punctuation_and_limits() {
    let words = vec![
        timed("one", 0, 200),
        timed("two", 250, 400),
        timed("three.", 450, 600),
        timed("four", 1_000, 1_200), // 400 ms pause
        timed("five", 1_250, 1_400),
    ];
    let options = PageOptions { max_words: 4, max_chars: 40, ..PageOptions::default() };
    let pages = group_pages(&words, &options);
    let grouped: Vec<Vec<&str>> =
        pages.iter().map(|page| page.words.iter().map(|w| w.text.as_str()).collect()).collect();
    assert_eq!(grouped, vec![vec!["one", "two", "three."], vec!["four", "five"]]);
    assert_eq!(pages[0].start, Time::from_millis(0));
    assert_eq!(pages[0].end, Time::from_millis(600));
    assert_eq!(pages[1].start, Time::from_millis(1_000));

    // Word and character limits flush before the word that would overflow.
    let long = vec![timed("aaaa", 0, 100), timed("b", 150, 200), timed("c", 250, 300)];
    let strict = PageOptions {
        max_words: 2,
        max_chars: 5,
        pause_threshold: Time::from_millis(5_000),
        break_on_punctuation: false,
    };
    let pages = group_pages(&long, &strict);
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].words.len(), 1); // "aaaa b" would be 6 chars
    assert_eq!(pages[1].words.len(), 2);
}

proptest::proptest! {
    #[test]
    fn pages_never_split_words_or_exceed_limits(
        lengths in proptest::collection::vec(1usize..12, 1..40),
        gaps in proptest::collection::vec(0i64..500, 0..40)
    ) {
        let mut cursor = 0i64;
        let words: Vec<Word> = lengths
            .iter()
            .zip(gaps.iter().cycle())
            .map(|(length, gap)| {
                cursor += *gap;
                let start = cursor;
                cursor += 300;
                timed(&"x".repeat(*length), start, cursor)
            })
            .collect();
        let options = PageOptions { max_words: 6, max_chars: 20, ..PageOptions::default() };
        let pages = group_pages(&words, &options);
        let flattened: Vec<&Word> = pages.iter().flat_map(|page| page.words.iter()).collect();
        proptest::prop_assert_eq!(flattened.len(), words.len());
        for (actual, expected) in flattened.iter().zip(words.iter()) {
            proptest::prop_assert_eq!(*actual, expected);
        }
        for page in &pages {
            proptest::prop_assert!(page.words.len() <= 6);
            let chars: usize = page.words.iter().map(|word| word.text.chars().count()).sum();
            proptest::prop_assert!(chars <= 20 || page.words.len() == 1);
        }
    }
}

#[test]
fn snapshots_for_all_four_presets() {
    let transcript = parse_transcript(WHISPERX).unwrap();
    let render_preset = |preset| {
        ass::write(&render(&transcript.words, &KaraokeOptions::new(preset)).unwrap()).unwrap()
    };
    insta::assert_snapshot!("preset_hormozi", render_preset(KaraokePreset::Hormozi));
    insta::assert_snapshot!("preset_clean", render_preset(KaraokePreset::Clean));
    insta::assert_snapshot!("preset_karaoke", render_preset(KaraokePreset::Karaoke));
    insta::assert_snapshot!("preset_bounce", render_preset(KaraokePreset::Bounce));
}

#[test]
fn custom_options_snapshot() {
    let transcript = parse_transcript(WHISPERX).unwrap();
    let mut options = KaraokeOptions::new(KaraokePreset::Karaoke);
    options.font = "Montserrat".to_owned();
    options.font_size = 96;
    options.width = 720;
    options.height = 1280;
    options.safe_area.left = 40;
    options.safe_area.right = 40;
    options.safe_area.bottom = 260;
    options.emphasis = vec!["amazing".to_owned()];
    let text = ass::write(&render(&transcript.words, &options).unwrap()).unwrap();
    insta::assert_snapshot!("custom_options", text);
}

#[test]
fn presets_emit_their_signature_override_tags() {
    let transcript = parse_transcript(WHISPERX).unwrap();
    let rendered = |preset| {
        ass::write(&render(&transcript.words, &KaraokeOptions::new(preset)).unwrap()).unwrap()
    };

    let hormozi = rendered(KaraokePreset::Hormozi);
    assert!(hormozi.contains("HEY,"), "hormozi words are uppercased");
    assert!(hormozi.contains("\\t("), "hormozi animates words");
    assert!(hormozi.contains("\\fscx115"));

    let clean = rendered(KaraokePreset::Clean);
    let clean_style = style_fields(&clean);
    assert_eq!(clean_style[15], "3", "clean uses a box");
    assert!(clean.contains("\\alpha&H00&"), "clean reveals words");

    let karaoke = rendered(KaraokePreset::Karaoke);
    // The first word is spoken 0-320 ms, but its fill must run through the
    // 20 ms gap to the next word at 340 ms: 34 centiseconds, not 32. The old
    // 32 value made every subsequent fill drift ahead of the voice.
    assert!(karaoke.contains("\\k34"), "first fill covers 0-340 ms");

    let bounce = rendered(KaraokePreset::Bounce);
    assert!(bounce.contains("\\fscx135"));
    assert!(bounce.contains("\\fscx88"));
}

#[test]
fn emphasis_and_font_options_change_real_output() {
    let transcript = parse_transcript(WHISPERX).unwrap();
    let plain = ass::write(
        &render(&transcript.words, &KaraokeOptions::new(KaraokePreset::Bounce)).unwrap(),
    )
    .unwrap();
    let mut options = KaraokeOptions::new(KaraokePreset::Bounce);
    options.font = "Anton".to_owned();
    options.emphasis = vec!["amazing".to_owned()];
    let custom = ass::write(&render(&transcript.words, &options).unwrap()).unwrap();
    assert_eq!(style_fields(&plain)[1], "DejaVu Sans");
    assert_eq!(style_fields(&custom)[1], "Anton");
    assert!(!plain.contains(&options.emphasis_color.format_ass()));
    assert!(custom.contains(&options.emphasis_color.format_ass()));
    // The emphasised word keeps a stronger scale than an ordinary word.
    assert!(custom.contains("\\fscx145"));

    let mut karaoke_options = KaraokeOptions::new(KaraokePreset::Karaoke);
    karaoke_options.emphasis = vec!["amazing".to_owned()];
    let karaoke = ass::write(&render(&transcript.words, &karaoke_options).unwrap()).unwrap();
    assert!(karaoke.contains("\\kf"), "emphasis uses whole-word fill");
}

#[test]
fn rendered_ass_reparses_with_one_dialogue_per_page() {
    let transcript = parse_transcript(WHISPERX).unwrap();
    let document = render(&transcript.words, &KaraokeOptions::new(KaraokePreset::Hormozi)).unwrap();
    let text = ass::write(&document).unwrap();
    assert!(text.contains("PlayResX: 1080"));
    assert!(text.contains("PlayResY: 1920"));
    assert_eq!(style_fields(&text)[21], "420");
    let reparsed = ass::parse(&text).unwrap();
    assert_eq!(reparsed.dialogues().len(), 3);
    let (format, first) = reparsed.dialogues()[0];
    assert_eq!(first.start(format).unwrap(), Time::ZERO);
    assert!(first.text(format).unwrap().contains("HEY,"));
    assert!(first.text(format).unwrap().contains("\\pos(540,1500)"));
}

#[test]
fn sub_threshold_page_gaps_are_closed_without_blinking() {
    // Page one holds four words spaced 60 ms apart; page two starts 60 ms
    // after page one ends. Without gap closing, the captions blink twice.
    let words = vec![
        timed("hoje", 0, 340),
        timed("eu", 400, 740),
        timed("vou", 800, 1_140),
        timed("te", 1_200, 1_540),
        timed("mostrar", 1_600, 1_940),
    ];
    let options = KaraokeOptions::new(KaraokePreset::Hormozi);
    let document = render(&words, &options).unwrap();
    let reparsed = ass::parse(&ass::write(&document).unwrap()).unwrap();
    let dialogues = reparsed.dialogues();
    assert!(dialogues.len() >= 2, "expected at least two pages, got {}", dialogues.len());
    for pair in dialogues.windows(2) {
        let (format_a, event_a) = pair[0];
        let (format_b, event_b) = pair[1];
        let end = event_a.end(format_a).unwrap();
        let start = event_b.start(format_b).unwrap();
        assert_eq!(end, start, "consecutive pages must be contiguous, found {end} -> {start}");
    }
    assert_eq!(dialogues[0].1.end(dialogues[0].0).unwrap(), Time::from_millis(1_600));

    // Disabling the threshold keeps the original 60 ms hole.
    let mut disabled = KaraokeOptions::new(KaraokePreset::Hormozi);
    disabled.page_gap_threshold = Time::ZERO;
    let reparsed = ass::parse(&ass::write(&render(&words, &disabled).unwrap()).unwrap()).unwrap();
    assert_eq!(
        reparsed.dialogues()[0].1.end(reparsed.dialogues()[0].0).unwrap(),
        Time::from_millis(1_540)
    );
}

#[test]
fn override_markup_is_escaped_and_emoji_passes_through() {
    let words = vec![Word {
        text: "a {\\b1}b 👋".to_owned(),
        start: Time::ZERO,
        end: Time::from_millis(500),
        score: None,
    }];
    let text =
        ass::write(&render(&words, &KaraokeOptions::new(KaraokePreset::Clean)).unwrap()).unwrap();
    assert!(text.contains("｛\\b1｝"));
    assert!(!text.contains("{\\b1}"));
    assert!(text.contains("👋"));
}

fn strip_ass_tags(text: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '{' => in_tag = true,
            '}' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
}

fn dialogue_lines(text: &str) -> Vec<Vec<String>> {
    ass::parse(text)
        .unwrap()
        .dialogues()
        .into_iter()
        .map(|(format, event)| {
            strip_ass_tags(event.text(format).unwrap()).split("\\N").map(str::to_owned).collect()
        })
        .collect()
}

#[test]
fn every_generated_line_fits_the_safe_width() {
    let transcript = parse_transcript(REPRO).unwrap();
    assert_eq!(transcript.words.len(), 14);
    let options = KaraokeOptions::new(KaraokePreset::Hormozi);
    let safe_width = f64::from(options.width - options.safe_area.left - options.safe_area.right);
    let text = ass::write(&render(&transcript.words, &options).unwrap()).unwrap();
    let pages = dialogue_lines(&text);

    assert!(pages.len() >= 4, "expected several pages, got {}", pages.len());
    assert!(pages.iter().any(|lines| lines.len() == 2), "expected at least one two-line page");
    for lines in &pages {
        assert!((1..=2).contains(&lines.len()), "page has {} lines", lines.len());
        let words: usize = lines.iter().map(|line| line.split_whitespace().count()).sum();
        assert!((1..=4).contains(&words), "page has {words} words: {lines:?}");
        for line in lines {
            let width =
                estimate_text_width(line, options.font_size, options.preset.uppercases_text());
            assert!(width <= safe_width, "line {line:?} is {width}px > {safe_width}px");
        }
    }
}

#[test]
fn transform_times_are_milliseconds_and_k_is_centiseconds() {
    let transcript = parse_transcript(WHISPERX).unwrap();
    let render_preset = |preset| {
        ass::write(&render(&transcript.words, &KaraokeOptions::new(preset)).unwrap()).unwrap()
    };

    // First word: 0-320 ms. The highlight must be held across that whole
    // window, not fire in centiseconds during the first 16 ms.
    let hormozi = render_preset(KaraokePreset::Hormozi);
    assert!(hormozi.contains("\\t(0,1,\\"), "instant highlight-on at 0 ms");
    assert!(hormozi.contains("\\t(320,321,\\"), "highlight released at 320 ms");
    assert!(hormozi.contains("\\t(0,80,\\fscx115"), "80 ms pop-in");
    assert!(hormozi.contains("\\t(80,160,\\fscx100"), "scale back after 160 ms");

    let clean = render_preset(KaraokePreset::Clean);
    assert!(clean.contains("\\t(0,1,\\"), "clean reveal starts at 0 ms");
    assert!(clean.contains("\\t(320,321,\\"), "clean reveal ends at 320 ms");

    let bounce = render_preset(KaraokePreset::Bounce);
    assert!(bounce.contains("\\t(0,80,\\fscx135"), "bounce pop-in uses milliseconds");
    assert!(bounce.contains("\\t(320,321,\\"), "bounce highlight released at 320 ms");

    // \kf/\k are the one place where centiseconds are correct. The first
    // fill spans 0-340 ms (spoken 0-320 ms plus the gap to the next word),
    // so it is 34 cs and lines up with the \t(340) transform of word two.
    let karaoke = render_preset(KaraokePreset::Karaoke);
    assert!(karaoke.contains("\\k34"), "first fill covers 0-340 ms");
    assert!(karaoke.contains("\\t(0,80,\\fscx108"), "karaoke pop-in uses milliseconds");
}
