use infer;
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use url_escape::decode;
use walkdir::WalkDir;

fn main() {
    let args: Vec<String> = env::args().collect();
    let root_dir = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        env::current_dir().expect("Failed to get current directory")
    };

    let listener = TcpListener::bind("127.0.0.1:8080").expect("Could not bind to port 8080");
    println!("Server listening on port 8080");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_connection(stream, &root_dir),
            Err(e) => eprintln!("Failed to establish a connection: {}", e),
        }
    }
}

fn handle_connection(mut stream: TcpStream, root_dir: &Path) {
    let mut buffer = [0; 1024];
    if stream.read(&mut buffer).is_err() {
        eprintln!("Failed to read from stream");
        return;
    }

    let request = String::from_utf8_lossy(&buffer[..]);
    let path = parse_request(&request, root_dir);

    if path.is_dir() {
        serve_directory(&path, root_dir, &mut stream);
    } else if path.is_file() {
        serve_file(&path, &mut stream);
    } else {
        respond_404(&mut stream);
    }
}

fn parse_request(request: &str, root_dir: &Path) -> PathBuf {
    let request_line = request.lines().next().expect("Failed to read request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("Failed to parse path");
    let decoded_path = decode(path).to_string();

    let resource = root_dir.join(decoded_path.trim_start_matches('/'));

    if resource.starts_with(root_dir) {
        resource
    } else {
        root_dir.to_path_buf() // Default to root directory if path is outside root
    }
}

fn serve_directory(path: &Path, root_dir: &Path, stream: &mut TcpStream) {
    let mut begin_html = r#"
    <!DOCTYPE html> 
    <html> 
    <head> 
        <meta charset="utf-8"> 
        <style>
            body { font-family: Arial, sans-serif; }
            ul { list-style-type: none; padding: 0; }
            li { margin: 5px 0; }
            a { text-decoration: none; color: #0366d6; }
            a:hover { text-decoration: underline; }
        </style>
    </head> 
    <body>"#
        .to_string();

    let relative_path = path.strip_prefix(root_dir).unwrap_or(path);
    let header = if relative_path.as_os_str().is_empty() {
        format!("<h1>Directory listing for {}</h1>", root_dir.display())
    } else {
        format!(
            "<h1>Directory listing for {}/{}</h1>",
            root_dir.display(),
            relative_path.display()
        )
    };
    begin_html.push_str(&header);

    let mut body = String::new();
    body.push_str("<ul>");

    // Always display "Go back up a directory" even at root
    let parent_url: String = if path == root_dir {
        "/".to_string() // At root, link just reloads the root
    } else if let Some(parent) = path.parent() {
        if parent.starts_with(root_dir) {
            let parent_display = parent
                .strip_prefix(root_dir)
                .unwrap_or(parent)
                .display()
                .to_string();
            url_escape::encode_query(&format!("/{}", parent_display)).to_string()
        } else {
            "/".to_string() // If for any reason parent is outside root, go back to "/"
        }
    } else {
        "/".to_string() // Fallback in case of unexpected errors
    };

    body.push_str(&format!(
        "<li><a href=\"{}\">⬆️ Go back up a directory</a></li>",
        parent_url
    ));

    // List current directory entries
    for entry in WalkDir::new(path)
        .min_depth(1)
        .max_depth(1)
        .sort_by_file_name()
    {
        if let Ok(entry) = entry {
            let entry_path = entry.path();
            let relative_path = entry_path.strip_prefix(root_dir).unwrap_or(entry_path);
            let entry_name = entry_path.file_name().unwrap_or_default().to_string_lossy();
            let entry_type = if entry_path.is_dir() {
                "📁 "
            } else {
                "📄 "
            };
            body.push_str(&format!(
                "<li>{}<a href=\"/{}\">{}</a></li>",
                entry_type,
                url_escape::encode_query(&relative_path.to_string_lossy()),
                entry_name
            ));
        }
    }
    body.push_str("</ul>");

    let end_html = r#"
    </body>
    </html>"#
        .to_string();

    let response_body = format!("{}{}{}", begin_html, body, end_html);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
        response_body.len(),
        response_body
    );

    stream.write_all(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn serve_file(path: &Path, stream: &mut TcpStream) {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => {
            respond_404(stream);
            return;
        }
    };

    let mut content = Vec::new();
    if file.read_to_end(&mut content).is_err() {
        respond_500(stream);
        return;
    }

    // Try to infer the MIME type using the `infer` crate
    let mime_type = infer::get(&content)
        .map(|t| t.mime_type().to_string())
        .unwrap_or_else(|| "text/plain".to_string()); // Default to text/plain if unable to infer

    // Automatically set to text/plain for unrecognized file extensions
    let is_text = mime_type.starts_with("text/")
        || mime_type == "application/json"
        || mime_type == "image/jpeg"
        || mime_type == "image/png"
        || mime_type == "image/gif"
        || mime_type == "application/pdf"
        || path.extension().and_then(|ext| ext.to_str()) == Some("rs")  // Rust files
        || path.extension().and_then(|ext| ext.to_str()) == Some("toml") // TOML files
        || path.extension().and_then(|ext| ext.to_str()) == Some("lock") // Lock files
        || mime_type == "text/plain"; // Default fallback for unknown types

    // Set Content-Type for Rust, TOML, and lock files as plain text
    let custom_mime_type = if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
        || path.extension().and_then(|ext| ext.to_str()) == Some("toml")
        || path.extension().and_then(|ext| ext.to_str()) == Some("lock")
    {
        "text/plain"
    } else {
        &mime_type
    };

    // Send the appropriate headers and content
    let response_header = if is_text {
        // For text, images, PDFs, Rust, TOML, and lock files, display them directly in the browser
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
            custom_mime_type,
            content.len()
        )
    } else {
        // For other file types (e.g., binary files), prompt the download
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
            mime_type,
            content.len()
        )
    };

    // Write the header and content to the stream
    if stream.write_all(response_header.as_bytes()).is_err() {
        return; // Unable to send response header
    }

    if stream.write_all(&content).is_err() {
        return; // Unable to send file content
    }

    stream.flush().unwrap_or(());
}

fn respond_404(stream: &mut TcpStream) {
    let response = "HTTP/1.1 404 NOT FOUND\r\n\r\n";
    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn respond_500(stream: &mut TcpStream) {
    let response = "HTTP/1.1 500 INTERNAL SERVER ERROR\r\n\r\nUnable to read file";
    stream.write_all(response.as_bytes()).unwrap_or(());
}
