use std::{fs,io::{BufReader,prelude::*},net::{TcpListener,TcpStream},collections::HashMap,};
use rusqlite::{Connection,params,};
use lingua::{Language, LanguageDetector, LanguageDetectorBuilder,};
use lingua::Language::{Spanish, English,};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:7878").unwrap();
    let mut conn = Connection::open("words.db").unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS words (
            id integer primary key autoincrement,
            word text not null unique
         )",
         (),
    ).unwrap();

    for stream in listener.incoming() {
        let stream = stream.unwrap();

        handle_connection(stream, &mut conn);
    }
}

fn handle_connection(mut stream: TcpStream, conn: &mut Connection) {
    let mut reader = BufReader::new(&stream);

    // Read request line
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();

    println!("line: {request_line:#?}");

    let (status_line, filename) = if request_line.contains("GET / HTTP/1.1") {
        ("HTTP/1.1 200 OK", "welcome.html")
    } else if request_line.contains("POST /save HTTP/1.1") {
        ("HTTP/1.1 200 OK", "saved_page.html")
    } else if request_line.contains("GET /style.css HTTP/1.1") {
        ("HTTP/1.1 200 OK", "style.css")
    } else {
        ("HTTP/1.1 400 NOT FOUND", "404.html")
    };

    // User is requesting to save word or clear the database 
    if filename == "saved_page.html" {
        let mut content_length = 0usize;

        // Parse content length
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header == "\r\n" {
                break;
            }
            if let Some(len) = header.strip_prefix("Content-Length: ") {
                content_length = len.trim().parse().unwrap();
            }
        }

        // Read body
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).unwrap();
        let body = String::from_utf8_lossy(&body);
        println!("Form body: {}", body);

        // Parse form fields
        let params = parse_form(&body);
        let newword = params.get("newword");
        if newword == None {
            conn.execute("DROP TABLE IF EXISTS words", ()).unwrap();
            write_word_count_html("saved_page.html", 0, String::from("Word count restarted!"));
        } else {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS words (
                    id integer primary key autoincrement,
                    word text not null unique
                 )",
                 (),
            ).unwrap();
            detect_and_save(newword.unwrap(), conn);
            let w = newword.unwrap();
            println!("Word: {w}");
        }
    }

    // Respond
    let contents = fs::read_to_string(filename).unwrap();
    let length = contents.len();
    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");

    stream.write_all(response.as_bytes()).unwrap();
}

fn detect_and_save(newword: &String, conn: &mut Connection) {
    let languages = vec![Spanish, English];
    let detector: LanguageDetector = LanguageDetectorBuilder::from_languages(&languages).build();
    let detected_language: Option<Language> = detector.detect_language_of(newword);
    let mut save_newword: bool = false;
    let mut user_message_update: Option<String>;

    match detected_language {
        Some(Spanish) => {
            let confidence = detector.compute_language_confidence(newword, Spanish);
            let rounded_confidence = (confidence * 100.0).round() / 100.0;
            println!("Good job! The word '{newword}' is Spanish!");
            user_message_update = Some(format!("Good job! The word '{}' is Spanish!", newword));
            println!("Confidence value = {rounded_confidence}");
            if rounded_confidence > 0.5 {
                save_newword = true;
            }
        }
        Some(English) => {
            println!("Warning: '{newword}' is English!");
            user_message_update = Some(format!("Oops! The word '{}' is English!", newword));
            let confidence = detector.compute_language_confidence(newword, Spanish);
            let rounded_confidence = (confidence * 100.0).round() / 100.0;
            println!("Confidence value = {rounded_confidence}");
        }
        None => {
            user_message_update = Some(format!("Uh oh! The word '{}' is not English or Spanish!", newword));
            println!("Uh oh! The word '{newword}' is not English or Spanish!");
        }
    }

    if save_newword {
        let _conn_res = conn.execute(
            "INSERT INTO words (word) VALUES (?1)",
            params![newword.to_lowercase()],
        );
        match _conn_res {
            Ok(_conn_res) => println!("Success"),
            Err(error) => {
                println!("Warning: Unique words only {:?}", error);
                user_message_update = Some(format!("Uh oh! The word '{}' has already been counted!", newword));

            }
        }
        println!("INSERTED ({:?}) to database.", newword.to_lowercase());
    }

    let count = count_entries(&conn);
    println!("Number of words known: {count}");
    if user_message_update != None {
        write_word_count_html("saved_page.html", count, user_message_update.unwrap());
    } else {
        write_word_count_html("saved_page.html", count, String::from("Oops...something went wrong"));
    }
}

fn write_word_count_html(path: &str, count: i64, user_message_update: String) {
    let mut file = match fs::File::create(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create HTML file '{}': {}", path, e);
            return;
        }
    };

    if let Err(e) = writeln!(
        file,
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>Word Count</title>
    <link rel="stylesheet" href="style.css">
  </head>
  <body>
    <h1 style="font-weight:bold;text-align:center;background-color:powderblue;border:3px solid black;">Word Count!
    <p>Track how many words you know in a foreign language</p>
    </h1>
    <h2>Motivation: "~2000 words + rules = fluency"</h2>
      <form action="/save" method="post">
        <label style="font-size:30px;">Enter word:</label><br>
        <input style="font-size:30px;" type="text" name="newword">
        <button style="font-size:30px;" type="submit">Save</button>
      </form>
    <body>
    <h2> {} </h2>
    <div class="container">
        <div class="box">Word count: <br><br>{}</div>
        <div class="box">Word of the day:</div>
        <div class="box">% nouns: <br>% adj: <br>%verbs:</div>
    </div>
    </body>
    <h3>
      <form action="/save" method="post">
        <button style="font-size:30px;" name="clear" type="submit">Clear Word Count</button>
      </form>
    </h3>
  </body>
</html>"#,
        user_message_update,
        count
    ) {
        eprintln!("Failed to write HTML file '{}': {}", path, e);
    }
}

fn count_entries(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM words",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next()?;
            Some((
                url_decode(key),
                url_decode(val),
            ))
        })
        .collect()
}

fn url_decode(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars();

    while let Some(c) = chars.next() {
        match c {
            '+' => out.push(' '),
            '%' => {
                let h1 = chars.next().unwrap();
                let h2 = chars.next().unwrap();
                let byte = u8::from_str_radix(&format!("{h1}{h2}"), 16).unwrap();
                out.push(byte as char);
            }
            _ => out.push(c),
        }
    }

    out
}


