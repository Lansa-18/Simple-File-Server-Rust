# Objectives
To build a simple file server that that can serve files as a simple HTML document listing all directories and folders as links.

## Setting up the project
- clone the repo
```
  git clone https://github.com/Lansa-18/Simple-File-Server-Rust
```
- Open the cloned project in your desired editor

## Starting up the server
There are 2 ways to start up the server
- `Cargo run`: This starts up the server at that current directory
- `cargo run /path/to/desired/directory`: This starts up the server at the particular path that was specified.
- Open `http://127.0.0.1:8080/` in your browser to view the server at whichever directory was specified.

