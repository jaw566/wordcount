use axum::{
    extract::{Form, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;
use std::sync::Arc;
use tower_http::services::ServeFile;
use lingua::{LanguageDetector, LanguageDetectorBuilder};
use lingua::Language::{Spanish, English};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    detector: Arc<LanguageDetector>,
}

#[tokio::main]
async fn main() {
    println!("Starting server...");

    let options = SqliteConnectOptions::from_str("sqlite:words.db")
        .unwrap()
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .unwrap_or_else(|_| {
            panic!("Failed to create pool");
        });

    // Initialize database
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL
        );"
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            user_id INTEGER NOT NULL,
            FOREIGN KEY(user_id) REFERENCES users(id)
        );"
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS words (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            word TEXT NOT NULL,
            word_language TEXT DEFAULT 'es',
            spanish_definition TEXT,
            english_definition TEXT,
            UNIQUE(user_id, word),
            FOREIGN KEY(user_id) REFERENCES users(id)
        );"
    )
    .execute(&pool)
    .await
    .unwrap();

    let languages = vec![Spanish, English];
    let detector = Arc::new(LanguageDetectorBuilder::from_languages(&languages).build());

    let state = AppState { db: pool, detector };

    let app = Router::new()
        .route("/", get(welcome_page))
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/app", get(dashboard))
        .route("/save", post(save_word))
        .route("/clear", post(clear_words))
        .route("/debug", get(debug_def))
        .fallback_service(ServeFile::new("style.css"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:7878").await.unwrap();
    println!("Listening on http://127.0.0.1:7878");
    axum::serve(listener, app).await.unwrap();
}

async fn get_user_id_from_session(jar: &CookieJar, pool: &SqlitePool) -> Option<i64> {
    if let Some(cookie) = jar.get("session_token") {
        let token = cookie.value();
        let record_result = sqlx::query("SELECT user_id FROM sessions WHERE token = ?")
            .bind(token)
            .fetch_optional(pool)
            .await
            .ok()??;
        
        let user_id: i64 = sqlx::Row::get(&record_result, "user_id");
        Some(user_id)
    } else {
        None
    }
}

async fn welcome_page(jar: CookieJar, State(state): State<AppState>) -> impl IntoResponse {
    if get_user_id_from_session(&jar, &state.db).await.is_some() {
        return Redirect::to("/app").into_response();
    }

    let html = r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>Word Count - Login</title>
    <link rel="stylesheet" href="style.css">
    <style> body {background-color:#f5f5f5; margin:0; padding: 2rem;} .form-box {background: white; padding: 2rem; border-radius: 8px; max-width: 400px; margin: 1rem auto; box-shadow: 0 4px 6px rgba(0,0,0,0.1);} </style>
  </head>
  <body>
    <h1 style="text-align:center;">Welcome to Word Count</h1>
    <p style="text-align:center;">Track how many words you know in a foreign language.</p>
    
    <div class="form-box">
      <h2>Login</h2>
      <form action="/login" method="post">
        <label>Username:</label><br>
        <input type="text" name="username" required><br><br>
        <label>Password:</label><br>
        <input type="password" name="password" required><br><br>
        <button type="submit">Login</button>
      </form>
    </div>

    <div class="form-box">
      <h2>Register</h2>
      <form action="/register" method="post">
        <label>Username:</label><br>
        <input type="text" name="username" required><br><br>
        <label>Password:</label><br>
        <input type="password" name="password" required><br><br>
        <button type="submit">Register</button>
      </form>
    </div>
  </body>
</html>"#;
    Html(html).into_response()
}

#[derive(Deserialize)]
struct AuthForm {
    username: String,
    password: String,
}

async fn register(State(state): State<AppState>, Form(form): Form<AuthForm>) -> impl IntoResponse {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hashed_password = match argon2.hash_password(form.password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to hash password").into_response(),
    };

    let res = sqlx::query(
        "INSERT INTO users (username, password_hash) VALUES (?, ?)")
        .bind(&form.username)
        .bind(&hashed_password)
    .execute(&state.db)
    .await;

    match res {
        Ok(_) => Html(r#"<h2>Registration successful! <a href="/">Go to Login</a></h2>"#).into_response(),
        Err(_) => Html(r#"<h2>Username already exists! <a href="/">Go back</a></h2>"#).into_response(),
    }
}

async fn login(jar: CookieJar, State(state): State<AppState>, Form(form): Form<AuthForm>) -> impl IntoResponse {
    let user = sqlx::query("SELECT id, password_hash FROM users WHERE username = ?")
        .bind(&form.username)
        .fetch_optional(&state.db)
        .await
        .unwrap();

    if let Some(user) = user {
        let password_hash: String = sqlx::Row::get(&user, "password_hash");
        let user_id: i64 = sqlx::Row::get(&user, "id");
        
        let parsed_hash = PasswordHash::new(&password_hash).unwrap();
        if Argon2::default().verify_password(form.password.as_bytes(), &parsed_hash).is_ok() {
            let session_token = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO sessions (token, user_id) VALUES (?, ?)")
                .bind(&session_token)
                .bind(user_id)
            .execute(&state.db)
            .await
            .unwrap();

            let cookie = Cookie::build(("session_token", session_token))
                .path("/")
                .http_only(true)
                .build();
            
            return (jar.add(cookie), Redirect::to("/app")).into_response();
        }
    }

    Html(r#"<h2>Invalid credentials! <a href="/">Go back</a></h2>"#).into_response()
}

async fn logout(jar: CookieJar, State(state): State<AppState>) -> impl IntoResponse {
    if let Some(cookie) = jar.get("session_token") {
        let token = cookie.value();
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&state.db)
            .await;
    }
    (jar.remove(Cookie::from("session_token")), Redirect::to("/")).into_response()
}

fn build_dashboard_html(count_es: i64, count_en: i64, words_html: &str, user_message: &str, active_bank: &str) -> String {
    let (es_style, en_style) = if active_bank == "en" {
        ("background:#eee; color:#555;", "background:white; border-bottom:2px solid #2196F3; font-weight:bold; color:#2196F3;")
    } else {
        ("background:white; border-bottom:2px solid #4CAF50; font-weight:bold; color:#4CAF50;", "background:#eee; color:#555;")
    };
    let bank_label = if active_bank == "en" { "English" } else { "Spanish" };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>Word Count App</title>
    <link rel="stylesheet" href="style.css">
    <style> 
      body {{background-color:#f5f5f5; margin:0; padding:10px; font-family: sans-serif;}} 
      .container {{ display: flex; gap: 20px; flex-wrap: wrap; margin-top: 20px; }}
      .box {{ background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); flex: 1; min-width: 200px; }}
      .word-list {{ margin-top: 20px; background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
      .word-item {{ display: flex; border-bottom: 1px solid #eee; padding: 10px 0; align-items: flex-start; }}
      .word-word {{ font-weight: bold; min-width: 130px; flex-shrink: 0; }}
      .word-def {{ color: #555; }}
      .tabs {{ display:flex; gap: 4px; margin-bottom: 16px; }}
      .tab {{ padding: 8px 20px; cursor:pointer; border: 1px solid #ccc; border-radius: 6px 6px 0 0; text-decoration:none; transition: all 0.15s; }}
    </style>
  </head>
  <body>
    <div style="display:flex; justify-content: space-between; align-items: center;">
      <h1 style="font-weight:bold;">Word Count!</h1>
      <form action="/logout" method="post" style="margin:0;">
          <button type="submit">Logout</button>
      </form>
    </div>
    <p>Track how many words you know in a foreign language</p>
    <h2>Motivation: "~2000 words + rules = fluency"</h2>
    
    <div style="background: white; padding: 20px; border-radius: 8px; display:inline-block;">
      <form action="/save" method="post">
        <label style="font-size:24px;">Enter word:</label><br>
        <input style="font-size:24px; margin: 10px 0;" type="text" name="newword" required autofocus><br>
        <button style="font-size:24px;" type="submit">Save</button>
      </form>
    </div>

    <h2 style="color: #d32f2f;">{}</h2>

    <div class="container">
        <div class="box"><h3 style="margin-top:0;">Spanish Bank</h3><div style="font-size: 48px; font-weight: bold; color: #4CAF50;">{}</div></div>
        <div class="box"><h3 style="margin-top:0;">English Bank</h3><div style="font-size: 48px; font-weight: bold; color: #2196F3;">{}</div></div>
        <div class="box">
            <h3 style="margin-top:0;">Danger Zone</h3>
            <form action="/clear" method="post">
                <button type="submit" style="background:#f44336; color:white; border:none; padding:10px 20px; font-size:16px; border-radius: 4px; cursor: pointer;">Clear My Word Count</button>
            </form>
        </div>
    </div>

    <div class="word-list">
        <div class="tabs">
            <a href="/app?bank=es" class="tab" style="{}">Spanish Bank</a>
            <a href="/app?bank=en" class="tab" style="{}">English Bank</a>
        </div>
        <h3 style="margin-top:0;">Your {} Words</h3>
        {}
    </div>
  </body>
</html>"#,
        user_message, count_es, count_en, es_style, en_style, bank_label, words_html
    )
}

async fn dashboard(jar: CookieJar, State(state): State<AppState>, axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
    let user_id = match get_user_id_from_session(&jar, &state.db).await {
        Some(id) => id,
        None => return Redirect::to("/").into_response(),
    };

    let active_bank = match params.get("bank").map(|s| s.as_str()) {
        Some("en") => "en",
        _ => "es",
    };

    let count_es_row = sqlx::query("SELECT COUNT(*) FROM words WHERE user_id = ? AND word_language = 'es'")
        .bind(user_id).fetch_one(&state.db).await.unwrap();
    let count_es: i64 = sqlx::Row::get(&count_es_row, 0);

    let count_en_row = sqlx::query("SELECT COUNT(*) FROM words WHERE user_id = ? AND word_language = 'en'")
        .bind(user_id).fetch_one(&state.db).await.unwrap();
    let count_en: i64 = sqlx::Row::get(&count_en_row, 0);

    let words = sqlx::query("SELECT word, word_language, spanish_definition, english_definition FROM words WHERE user_id = ? AND word_language = ? ORDER BY id DESC")
        .bind(user_id)
        .bind(active_bank)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let mut words_html = String::new();
    if words.is_empty() {
        words_html.push_str("<p>No words in this bank yet! Add some words above.</p>");
    } else {
        for record in words {
            let word: String = sqlx::Row::get(&record, "word");
            let lang: String = sqlx::Row::get(&record, "word_language");
            let def = if lang == "en" {
                let text: Option<String> = sqlx::Row::get(&record, "english_definition");
                text.unwrap_or_else(|| "No definition found.".to_string())
            } else {
                let text: Option<String> = sqlx::Row::get(&record, "spanish_definition");
                text.unwrap_or_else(|| "No definition found.".to_string())
            };
            words_html.push_str(&format!(
                r#"<div class="word-item">
                    <div class="word-word">{}</div>
                    <div class="word-def">{}</div>
                   </div>"#,
                word, def
            ));
        }
    }

    Html(build_dashboard_html(count_es, count_en, &words_html, "", active_bank)).into_response()
}

#[derive(Deserialize)]
struct WordForm {
    newword: String,
}


async fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
       .replace("&lt;", "<")
       .replace("&gt;", ">")
       .replace("&nbsp;", " ")
       .replace("&#39;", "'")
       .replace("&quot;", "\"")
}

async fn fetch_definition(word: &str, is_spanish: bool) -> Option<String> {
    let client = reqwest::Client::builder()
        .user_agent("WordcountApp/1.0 (language learning app)")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    if is_spanish {
        // Wiktionary REST API returns structured JSON grouped by language code
        let url = format!("https://en.wiktionary.org/api/rest_v1/page/definition/{}", word);
        println!("Fetching Spanish def from Wiktionary REST: {}", url);

        if let Ok(res) = client.get(&url).send().await {
            if let Ok(json) = res.json::<serde_json::Value>().await {
                for lang_key in &["es", "en"] {
                    if let Some(parts) = json.get(lang_key).and_then(|v| v.as_array()) {
                        for part in parts {
                            if let Some(defs) = part.get("definitions").and_then(|d| d.as_array()) {
                                for def in defs {
                                    if let Some(def_html) = def.get("definition").and_then(|d| d.as_str()) {
                                        let clean = strip_html(def_html).await.trim().to_string();
                                        if clean.len() > 5 && !clean.starts_with('(') {
                                            println!("Wiktionary REST definition: {:?}", clean);
                                            return Some(clean);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        println!("No Spanish definition found");
    } else {
        // dictionaryapi.dev works well for English words
        let url = format!("https://api.dictionaryapi.dev/api/v2/entries/en/{}", word);
        println!("Fetching English def from dictionaryapi.dev: {}", url);

        if let Ok(res) = client.get(&url).send().await {
            if let Ok(json) = res.json::<serde_json::Value>().await {
                if let Some(entries) = json.as_array() {
                    for entry in entries {
                        if let Some(meanings) = entry.get("meanings").and_then(|m| m.as_array()) {
                            for meaning in meanings {
                                if let Some(defs) = meaning.get("definitions").and_then(|d| d.as_array()) {
                                    if let Some(first) = defs.first() {
                                        if let Some(def_str) = first.get("definition").and_then(|d| d.as_str()) {
                                            let clean = def_str.trim().to_string();
                                            if clean.len() > 5 {
                                                println!("DictionaryAPI definition: {:?}", clean);
                                                return Some(clean);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        println!("No English definition found");
    }
    None
}

async fn debug_def() -> impl IntoResponse {
    let def = fetch_definition("gato", true).await;
    Html(format!("Definition for gato: {:?}", def)).into_response()
}

async fn translate_es_to_en(text: &str) -> String {
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=es&tl=en&dt=t&q={}",
        urlencoding::encode(text)
    );
    let client = reqwest::Client::builder()
        .user_agent("Anonymous")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    if let Ok(res) = client.get(&url).send().await {
        if let Ok(json) = res.json::<serde_json::Value>().await {
            // translate_googleapis format: [ [ ["Translated text", "Original text" ...
            if let Some(arr) = json.as_array() {
                if let Some(first_arr) = arr.get(0).and_then(|v| v.as_array()) {
                    let mut translated = String::new();
                    for segment in first_arr {
                        if let Some(text_segment) = segment.get(0).and_then(|v| v.as_str()) {
                            translated.push_str(text_segment);
                        }
                    }
                    if !translated.is_empty() {
                        return translated;
                    }
                }
            }
        }
    }
    text.to_string() // Fallback to original text if translation fails
}

async fn save_word(jar: CookieJar, State(state): State<AppState>, Form(form): Form<WordForm>) -> impl IntoResponse {
    let user_id = match get_user_id_from_session(&jar, &state.db).await {
        Some(id) => id,
        None => return Redirect::to("/").into_response(),
    };

    let newword = form.newword.trim().to_lowercase();
    if newword.is_empty() {
        return Redirect::to("/app").into_response();
    }

    let detected_language = state.detector.detect_language_of(&newword);
    let (save_newword, is_spanish, mut user_message_update) = match detected_language {
        Some(Spanish) => {
            let confidence = state.detector.compute_language_confidence(&newword, Spanish);
            let rounded_confidence = (confidence * 100.0).round() / 100.0;
            if rounded_confidence > 0.5 {
                (true, true, format!("Saved '{}' in Spanish bank!", newword))
            } else {
                // Low confidence Spanish, save as English
                (true, false, format!("Saved '{}' in English bank (low Spanish confidence).", newword))
            }
        }
        Some(English) => {
            (true, false, format!("Saved '{}' in English bank!", newword))
        }
        None => {
            // Default to saving as Spanish if the language can't be determined
            (true, true, format!("Saved '{}' (language unclear, defaulting to Spanish bank).", newword))
        }
    };

    if save_newword {
        let word_lang = if is_spanish { "es" } else { "en" };

        let (spanish_def, english_def) = if is_spanish {
            let sp = fetch_definition(&newword, true).await;
            let en = if let Some(ref def_text) = sp {
                Some(translate_es_to_en(def_text).await)
            } else { None };
            (sp, en)
        } else {
            let en = fetch_definition(&newword, false).await;
            (None, en)
        };

        let res = sqlx::query(
            "INSERT INTO words (user_id, word, word_language, spanish_definition, english_definition) VALUES (?, ?, ?, ?, ?)")
            .bind(user_id)
            .bind(&newword)
            .bind(word_lang)
            .bind(&spanish_def)
            .bind(&english_def)
        .execute(&state.db)
        .await;

        if res.is_err() {
            user_message_update = format!("Uh oh! The word '{}' has already been counted!", newword);
        }
    }

    let active_bank = if is_spanish { "es" } else { "en" };

    let count_es_row = sqlx::query("SELECT COUNT(*) FROM words WHERE user_id = ? AND word_language = 'es'")
        .bind(user_id).fetch_one(&state.db).await.unwrap();
    let count_es: i64 = sqlx::Row::get(&count_es_row, 0);

    let count_en_row = sqlx::query("SELECT COUNT(*) FROM words WHERE user_id = ? AND word_language = 'en'")
        .bind(user_id).fetch_one(&state.db).await.unwrap();
    let count_en: i64 = sqlx::Row::get(&count_en_row, 0);

    let words = sqlx::query("SELECT word, word_language, spanish_definition, english_definition FROM words WHERE user_id = ? AND word_language = ? ORDER BY id DESC")
        .bind(user_id)
        .bind(active_bank)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let mut words_html = String::new();
    if words.is_empty() {
        words_html.push_str("<p>No words in this bank yet! Add some words above.</p>");
    } else {
        for record in words {
            let word: String = sqlx::Row::get(&record, "word");
            let lang: String = sqlx::Row::get(&record, "word_language");
            let def = if lang == "en" {
                let text: Option<String> = sqlx::Row::get(&record, "english_definition");
                text.unwrap_or_else(|| "No definition found.".to_string())
            } else {
                let text: Option<String> = sqlx::Row::get(&record, "spanish_definition");
                text.unwrap_or_else(|| "No definition found.".to_string())
            };
            words_html.push_str(&format!(
                r#"<div class="word-item">
                    <div class="word-word">{}</div>
                    <div class="word-def">{}</div>
                   </div>"#,
                word, def
            ));
        }
    }

    Html(build_dashboard_html(count_es, count_en, &words_html, &user_message_update, active_bank)).into_response()
}


async fn clear_words(jar: CookieJar, State(state): State<AppState>) -> impl IntoResponse {
    if let Some(user_id) = get_user_id_from_session(&jar, &state.db).await {
        let _ = sqlx::query("DELETE FROM words WHERE user_id = ?")
            .bind(user_id)
            .execute(&state.db)
            .await;
    }
    Redirect::to("/app").into_response()
}
