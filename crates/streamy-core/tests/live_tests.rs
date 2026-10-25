use std::env;
use streamy_core::net::client::SourceClient;
use streamy_core::net::uvox::UvoxClient;

#[test]
fn test_live_dnas2_handshake_env_gated() {
    let host = match env::var("STREAMY_TEST_HOST").or_else(|_| env::var("STREAMY_LIVE_HOST")) {
        Ok(h) => h,
        Err(_) => {
            println!("STREAMY_TEST_HOST not set — skipping live DNAS2 test");
            return;
        }
    };
    let port = env::var("STREAMY_TEST_PORT")
        .or_else(|_| env::var("STREAMY_LIVE_PORT"))
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8030);
    let password = match env::var("STREAMY_TEST_PASSWORD").or_else(|_| env::var("STREAMY_LIVE_PASSWORD")) {
        Ok(p) => p,
        Err(_) => {
            println!("STREAMY_TEST_PASSWORD not set — skipping live DNAS2 test");
            return;
        }
    };
    let sid = env::var("STREAMY_TEST_SID")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1);

    let mut client = UvoxClient::new(
        host,
        port,
        sid,
        password,
        "audio/mpeg".to_string(),
        44_100,
        1,
        None,
    );

    let res = client.connect(128, "Streamy Live Unit Test", "Speech");
    assert!(res.is_ok(), "Live handshake failed: {:?}", res);

    // Send a silence frame
    let frame = vec![0u8; 1024];
    assert!(client.send(&frame).is_ok());

    // Update metadata
    assert!(client.update_now_playing("Streamy Live Title").is_ok());

    client.close();
}
