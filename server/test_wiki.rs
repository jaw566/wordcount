use reqwest;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct WiktionaryResponse {
    query: Option<WiktionaryQuery>,
}

#[derive(Deserialize, Debug)]
struct WiktionaryQuery {
    pages: std::collections::HashMap<String, WiktionaryPage>,
}

#[derive(Deserialize, Debug)]
struct WiktionaryPage {
    extract: Option<String>,
}

#[tokio::main]
async fn main() {
    let word = "adios";
    let is_spanish = true;
    let lang_code = if is_spanish { "es" } else { "en" };
    let url = format!(
        "https://{}.wiktionary.org/w/api.php?action=query&prop=extracts&titles={}&format=json&explaintext=1&exsentences=2",
        lang_code, word
    );

    println!("Fetching url: {}", url);
    let client = reqwest::Client::new();
    match client.get(&url).send().await {
        Ok(res) => {
            let text = res.text().await.unwrap();
            println!("Raw JSON: {}", text);
            if let Ok(data) = serde_json::from_str::<WiktionaryResponse>(&text) {
                if let Some(query) = data.query {
                    for (_, page) in query.pages {
                        if let Some(extract) = page.extract {
                            println!("Raw extract: {:?}", extract);
                            let mut definition = String::new();
                            for line in extract.lines() {
                                let trimmed = line.trim();
                                if !trimmed.is_empty() && !trimmed.starts_with('=') {
                                    if trimmed.len() > 1 || !trimmed.chars().all(char::is_numeric) {
                                        definition = trimmed.to_string();
                                        break;
                                    }
                                }
                            }
                            println!("Parsed definition: {:?}", definition);
                        } else {
                            println!("No extract via 'page.extract'");
                        }
                    }
                } else {
                    println!("No query data");
                }
            } else {
                println!("Failed to parse JSON");
            }
        }
        Err(e) => println!("Reqwest error: {}", e),
    }
}
