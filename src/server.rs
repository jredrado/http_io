use crate::error::Result;
use crate::protocol::{CrLfStream, HttpBody, HttpMethod, HttpRequest, HttpResponse};
use std::io::{self, Write};
use std::net;

pub trait Listen {
    type stream: io::Read + io::Write;
    fn accept(&self) -> Result<Self::stream>;
}

impl Listen for net::TcpListener {
    type stream = net::TcpStream;
    fn accept(&self) -> Result<net::TcpStream> {
        let (stream, _) = net::TcpListener::accept(self)?;
        Ok(stream)
    }
}

pub trait HttpRequestHandler<I: io::Read> {
    fn get(&self, uri: &str, stream: HttpBody<&mut I>) -> Result<HttpResponse<Box<dyn io::Read>>>;
    fn put(&self, uri: &str, stream: HttpBody<&mut I>) -> Result<HttpResponse<Box<dyn io::Read>>>;
}

pub struct HttpServer<L: Listen, H: HttpRequestHandler<L::stream>> {
    connection_stream: L,
    request_handler: H,
}

impl<L: Listen, H: HttpRequestHandler<L::stream>> HttpServer<L, H> {
    pub fn new(connection_stream: L, request_handler: H) -> Self {
        HttpServer {
            connection_stream,
            request_handler,
        }
    }

    fn serve_one(&self) -> Result<()> {
        let mut stream = io::BufReader::new(self.connection_stream.accept()?);
        let mut ts = CrLfStream::new(&mut stream);
        let request = HttpRequest::deserialize(&mut ts)?;
        drop(ts);

        let headers = &request.headers;
        let encoding = headers.get("Transfer-Encoding");
        let content_length = headers.get("Content-Length").map(str::parse).transpose()?;

        let mut stream = stream.into_inner();
        let body = HttpBody::new(encoding, content_length, &mut stream);
        let mut response = match request.method {
            HttpMethod::Get => self.request_handler.get(&request.uri, body)?,
            HttpMethod::Put => self.request_handler.put(&request.uri, body)?,
        };
        write!(stream, "{}", response)?;
        io::copy(&mut response.body, &mut stream)?;

        Ok(())
    }

    pub fn serve_forever(&self) -> ! {
        loop {
            match self.serve_one() {
                Err(e) => println!("Error {:?}", e),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod client_server_tests {
    use super::{HttpRequestHandler, HttpServer};
    use crate::client::HttpClient;
    use crate::error::Result;
    use crate::protocol::{HttpBody, HttpResponse, HttpStatus};
    use std::{io, net, thread};

    struct TestRequestHandler();

    impl TestRequestHandler {
        fn new() -> Self {
            TestRequestHandler()
        }
    }

    impl<I: io::Read> HttpRequestHandler<I> for TestRequestHandler {
        fn get(
            &self,
            _uri: &str,
            _stream: HttpBody<&mut I>,
        ) -> Result<HttpResponse<Box<dyn io::Read>>> {
            Ok(HttpResponse::new(HttpStatus::OK, Box::new(io::empty())))
        }
        fn put(
            &self,
            _uri: &str,
            _stream: HttpBody<&mut I>,
        ) -> Result<HttpResponse<Box<dyn io::Read>>> {
            Ok(HttpResponse::new(HttpStatus::OK, Box::new(io::empty())))
        }
    }

    fn connected_client_server() -> (
        HttpClient<net::TcpStream>,
        HttpServer<net::TcpListener, TestRequestHandler>,
    ) {
        let server_socket = net::TcpListener::bind("localhost:0").unwrap();
        let server_address = server_socket.local_addr().unwrap();
        let handler = TestRequestHandler::new();
        let server = HttpServer::new(server_socket, handler);

        let client_socket = net::TcpStream::connect(server_address).unwrap();
        let client = HttpClient::new(client_socket);

        (client, server)
    }

    #[test]
    fn request_one() {
        let (client, server) = connected_client_server();
        let handle = thread::spawn(move || server.serve_one());
        let response = client.get("localhost", "/").unwrap();
        handle.join().unwrap().unwrap();
        assert_eq!(response.status, HttpStatus::OK);
    }
}