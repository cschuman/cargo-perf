// perf-guard: async-block-in-async
// Negative (D2): a local `mod net` with its own `TcpStream::connect` is not
// std::net::TcpStream::connect; connecting to it does not block the runtime.
mod net {
    pub struct TcpStream;
    impl TcpStream {
        pub fn connect(_addr: &str) -> Self {
            TcpStream
        }
    }
}

async fn run() {
    let _ = net::TcpStream::connect("127.0.0.1:8080");
}
