pub mod types;
pub mod validation;
pub mod handlers;
pub mod state;

#[cfg(test)]
mod tests {
    use crate::types::{Envelope, Post};
    use chrono::Utc;
    use serde_json;

    #[test]
    fn test_envelope_serialization() {
        let envelope = Envelope {
            signature: "-----BEGIN PGP SIGNATURE-----\ntest_signature\n-----END PGP SIGNATURE-----".to_string(),
            public_key: "-----BEGIN PGP PUBLIC KEY BLOCK-----\ntest_key\n-----END PGP PUBLIC KEY BLOCK-----".to_string(),
            id: "2fef8ec4334abede9aeb1d40293f2d6dbcc1edd0".to_string(),
            data: r#"{"id":"2fef8ec4334abede9aeb1d40293f2d6dbcc1edd0","text":"test","latitude":33.5583,"longitude":-84.2541,"date":"2025-06-03T02:06:56.465Z"}"#.to_string(),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        println!("Envelope JSON: {}", json);

        let deserialized: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope.id, deserialized.id);
    }

    #[test]
    fn test_post_serialization() {
        let post = Post {
            id: "2fef8ec4334abede9aeb1d40293f2d6dbcc1edd0".to_string(),
            text: "Hello world!".to_string(),
            latitude: 33.7501,
            longitude: -84.3885,
            date: Utc::now(),
            parent: Some("8558e99c353bbac709e470b6342241c315fe352a".to_string()),
        };

        let json = serde_json::to_string(&post).unwrap();
        println!("Post JSON: {}", json);

        let deserialized: Post = serde_json::from_str(&json).unwrap();
        assert_eq!(post.id, deserialized.id);
        assert_eq!(post.text, deserialized.text);
    }
}
