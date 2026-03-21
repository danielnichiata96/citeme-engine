use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::types::{FormatOptions, OutputFormat};

fn main() {
    let mut engine = CitationEngine::new();
    let apa_csl = include_str!("../../../tests/fixtures/styles/apa.csl");
    let en_locale = include_str!("../../../tests/fixtures/locales/locales-en-US.xml");
    engine.load_style("apa", apa_csl).unwrap();
    engine.load_locale("en-US", en_locale).unwrap();

    let opts = FormatOptions { output_format: OutputFormat::Html, abnt_post_process: false };

    let inputs: Vec<&str> = vec![
        r#"{"type":"article-journal","id":"smith2024a","title":"A Study of Something Important","author":[{"family":"Smith","given":"John A."},{"family":"Doe","given":"Jane B."}],"issued":{"date-parts":[[2024]]},"container-title":"Journal of Important Studies","volume":"42","issue":"3","page":"100-115","DOI":"10.1234/jis.2024.001"}"#,
        r#"{"type":"article-journal","id":"jones2023","title":"Understanding Patterns","author":[{"family":"Jones","given":"Robert"}],"issued":{"date-parts":[[2023]]},"container-title":"Psychology Review","volume":"15","page":"45-67"}"#,
        r#"{"type":"article-journal","id":"team2024","title":"Large Scale Collaboration Study","author":[{"family":"Brown","given":"A."},{"family":"Green","given":"B."},{"family":"White","given":"C."},{"family":"Black","given":"D."},{"family":"Gray","given":"E."},{"family":"Blue","given":"F."}],"issued":{"date-parts":[[2024]]},"container-title":"Nature Methods","volume":"21","page":"1-20"}"#,
        r#"{"type":"book","id":"kwan2014","title":"Crazy Rich Asians","author":[{"family":"Kwan","given":"Kevin"}],"issued":{"date-parts":[[2014]]},"publisher":"Anchor Books","publisher-place":"New York"}"#,
        r#"{"type":"chapter","id":"lee2020","title":"Machine Learning Approaches","author":[{"family":"Lee","given":"Sarah"}],"issued":{"date-parts":[[2020]]},"container-title":"Handbook of AI","editor":[{"family":"Wang","given":"Li"}],"publisher":"MIT Press","page":"200-250"}"#,
        r#"{"type":"thesis","id":"garcia2022","title":"Neural Network Optimization","author":[{"family":"Garcia","given":"Maria"}],"issued":{"date-parts":[[2022]]},"publisher":"Stanford University","genre":"Doctoral dissertation"}"#,
        r#"{"type":"paper-conference","id":"chen2023","title":"Quantum Computing Advances","author":[{"family":"Chen","given":"Wei"}],"issued":{"date-parts":[[2023]]},"container-title":"Proceedings of IEEE Conference","publisher":"IEEE","page":"50-55"}"#,
        r#"{"type":"report","id":"who2024","title":"Global Health Report 2024","author":[{"literal":"World Health Organization"}],"issued":{"date-parts":[[2024]]},"publisher":"WHO Press"}"#,
        r#"{"type":"article-journal","id":"anon2023","title":"Anonymous Contribution to Science","issued":{"date-parts":[[2023]]},"container-title":"Science Weekly","volume":"5","page":"1-10"}"#,
        r#"{"type":"webpage","id":"wiki2024","title":"Citation Style Language","author":[{"literal":"Wikipedia contributors"}],"issued":{"date-parts":[[2024]]},"container-title":"Wikipedia","URL":"https://en.wikipedia.org/wiki/Citation_Style_Language"}"#,
    ];

    for (i, input) in inputs.iter().enumerate() {
        let result = engine.format_one(input, "apa", "en-US", &opts).unwrap();
        println!("--- Fixture {} ---", i+1);
        println!("ref: {}", result.reference);
        println!("cite: {}", result.in_text);
        println!();
    }
}
