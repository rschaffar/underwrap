use super::*;
use std::thread;
use underwrap_protocol::libsecret_publication::{
    REQUEST_HEADER_LEN, decode_request_header, encode_response,
};

fn host_once(mut stream: UnixStream, response: ResponseKind) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut header = [0; REQUEST_HEADER_LEN];
        stream.read_exact(&mut header).unwrap();
        let decoded = decode_request_header(&header).unwrap();
        let mut body = vec![0; decoded.body_len];
        stream.read_exact(&mut body).unwrap();
        stream
            .write_all(&encode_response(response, decoded.generation))
            .unwrap();
        body
    })
}

#[test]
fn publication_waits_for_exact_ack_and_rejects() {
    let (publisher, host) = UnixStream::pair().unwrap();
    let join = host_once(host, ResponseKind::Ack);
    let mut publisher = Publisher::from_stream(publisher);
    publisher
        .publish_section(&LibsecretSection::default())
        .unwrap();
    assert_eq!(
        join.join().unwrap(),
        LibsecretSection::default().to_canonical_bytes().unwrap()
    );

    let (publisher, host) = UnixStream::pair().unwrap();
    let join = host_once(host, ResponseKind::Reject);
    let mut publisher = Publisher::from_stream(publisher);
    assert!(
        publisher
            .publish_section(&LibsecretSection::default())
            .is_err()
    );
    join.join().unwrap();
}

#[test]
fn publication_result_is_blocked_at_exact_protocol_state_until_ack() {
    let (publisher, mut host) = UnixStream::pair().unwrap();
    let mut publisher = Publisher::from_stream(publisher);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let publish = thread::spawn(move || {
        result_tx
            .send(publisher.publish_section(&LibsecretSection::default()))
            .unwrap();
    });

    let mut header = [0; REQUEST_HEADER_LEN];
    host.read_exact(&mut header).unwrap();
    let decoded = decode_request_header(&header).unwrap();
    assert_eq!(decoded.generation, 0);
    let mut body = vec![0; decoded.body_len];
    host.read_exact(&mut body).unwrap();
    assert!(
        result_rx.try_recv().is_err(),
        "publication returned before host response"
    );
    host.write_all(&encode_response(ResponseKind::Ack, 0))
        .unwrap();
    result_rx.recv().unwrap().unwrap();
    publish.join().unwrap();
}

#[test]
fn publication_rejects_wrong_response_magic_version_tag_generation_partial_and_eof() {
    let valid = encode_response(ResponseKind::Ack, 0);
    let mut wrong_magic = valid;
    wrong_magic[0] ^= 0xff;
    let mut wrong_version = valid;
    wrong_version[4] ^= 0xff;
    let mut wrong_tag = valid;
    wrong_tag[5] = 0xff;
    for response in [
        Some(wrong_magic.to_vec()),
        Some(wrong_version.to_vec()),
        Some(wrong_tag.to_vec()),
        Some(encode_response(ResponseKind::Ack, 1).to_vec()),
        Some(valid[..3].to_vec()),
        None,
    ] {
        let (publisher, mut host) = UnixStream::pair().unwrap();
        let join = thread::spawn(move || {
            let mut header = [0; REQUEST_HEADER_LEN];
            host.read_exact(&mut header).unwrap();
            let decoded = decode_request_header(&header).unwrap();
            let mut body = vec![0; decoded.body_len];
            host.read_exact(&mut body).unwrap();
            if let Some(response) = response {
                host.write_all(&response).unwrap();
            }
        });
        let mut publisher = Publisher::from_stream(publisher);
        assert!(
            publisher
                .publish_section(&LibsecretSection::default())
                .is_err()
        );
        join.join().unwrap();
    }
}

#[test]
fn publication_request_write_and_response_read_failures_are_reported() {
    let (publisher, host) = UnixStream::pair().unwrap();
    drop(host);
    let mut publisher = Publisher::from_stream(publisher);
    let error = publisher
        .publish_section(&LibsecretSection::default())
        .unwrap_err();
    assert!(format!("{error:#}").contains("writing publication"));

    let (publisher, mut host) = UnixStream::pair().unwrap();
    publisher.set_nonblocking(true).unwrap();
    let mut publisher = Publisher::from_stream(publisher);
    let error = publisher
        .publish_section(&LibsecretSection::default())
        .unwrap_err();
    assert!(format!("{error:#}").contains("reading publication response"));
    let mut header = [0; REQUEST_HEADER_LEN];
    host.read_exact(&mut header).unwrap();
    let decoded = decode_request_header(&header).unwrap();
    let mut body = vec![0; decoded.body_len];
    host.read_exact(&mut body).unwrap();
    assert_eq!(decoded.generation, 0);
}
