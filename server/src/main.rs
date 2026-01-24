use std::{
    fs,
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
    collections::HashMap,
};
use rusqlite::{
    Connection,
    params,
};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:7878").unwrap();
    let mut conn = Connection::open("words.db").unwrap();

    conn.execute(
        "create table if not exists words (
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
        ("HTTP/1.1 200 OK", "words_page.html")
    } else if request_line.contains("POST /save HTTP/1.1") {
        ("HTTP/1.1 200 OK", "saved_page.html")
    } else {
        ("HTTP/1.1 400 NOT FOUND", "404.html")
    };

    // User is requesting to save word
    if filename == "saved_page.html" {
        // Read headers
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
        let newword = params.get("newword").unwrap();
        println!("Word: {newword}");

        let _conn_res = conn.execute(
            "INSERT INTO words (word) VALUES (?1)",
            params![newword],
        );

        match _conn_res {
            Ok(_conn_res) => println!("Success"),
            Err(error) => println!("Warning: Unique words only {:?}", error),
        }

        let count = count_entries(&conn);
        println!("Number of words known: {count}");
        write_word_count_html("saved_page.html", count);
    }

    // Respond
    let contents = fs::read_to_string(filename).unwrap();
    let length = contents.len();
    let response = 
        format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");

    stream.write_all(response.as_bytes()).unwrap();
}

fn write_word_count_html(path: &str, count: i64) {
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
    <title>Hello!</title>
  </head>
  <body>
    <h1>Saved!</h1>
    <h2>"~2000 words + rules = fluency"</h2>
      <form action="/save" method="post">
        <label>Enter word:</label><br>
        <input type="text" name="newword">
        <button type="submit">Save</button>
      </form>
    <h2>Number of words banked: {}</h2>
  </body>
</html>"#,
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


