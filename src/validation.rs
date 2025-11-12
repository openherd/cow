use crate::types::{Envelope, Post, ValidationError};
use pgp::{Deserializable, SignedPublicKey};
use pgp::types::KeyTrait;

pub fn validate_envelope(envelope: &Envelope) -> Result<Post, ValidationError> {
   
    validate_envelope_structure(envelope)?;
    
   
    let (public_key, _) = SignedPublicKey::from_string(&envelope.public_key)?;
    
   
    let fingerprint = hex::encode(public_key.fingerprint());
    if fingerprint.to_lowercase() != envelope.id.to_lowercase() {
        return Err(ValidationError::IdMismatch);
    }
    
   
    verify_signature(&envelope.signature, &envelope.data, &public_key)?;
    
   
    let post: Post = serde_json::from_str(&envelope.data)?;
    
   
    if post.id != envelope.id {
        return Err(ValidationError::InvalidPostData(
            "Post ID does not match envelope ID".to_string()
        ));
    }
    
   
    validate_post(&post)?;
    
    Ok(post)
}

fn verify_signature(
    signature_armored: &str,
    data: &str,
    public_key: &SignedPublicKey,
) -> Result<(), ValidationError> {
    use pgp::StandaloneSignature;
    
   
    let (signature, _) = StandaloneSignature::from_string(signature_armored)?;
    
   
    signature.verify(public_key, data.as_bytes())?;
    
    Ok(())
}

fn validate_envelope_structure(envelope: &Envelope) -> Result<(), ValidationError> {
   
    if envelope.signature.is_empty() {
        return Err(ValidationError::InvalidSignature);
    }
    
    if envelope.public_key.is_empty() {
        return Err(ValidationError::InvalidPublicKey);
    }
    
    if envelope.id.is_empty() {
        return Err(ValidationError::IdMismatch);
    }
    
    if envelope.data.is_empty() {
        return Err(ValidationError::InvalidPostData("Empty data field".to_string()));
    }
    
   
    if !envelope.signature.contains("-----BEGIN PGP SIGNATURE-----") {
        return Err(ValidationError::InvalidSignature);
    }
    
    if !envelope.public_key.contains("-----BEGIN PGP PUBLIC KEY BLOCK-----") {
        return Err(ValidationError::InvalidPublicKey);
    }
    
   
    if !envelope.id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ValidationError::IdMismatch);
    }
    
    Ok(())
}

fn validate_post(post: &Post) -> Result<(), ValidationError> {
   
    if post.text.trim().is_empty() {
        return Err(ValidationError::InvalidPostData(
            "Post text cannot be empty".to_string()
        ));
    }
    
   
    if post.latitude < -90.0 || post.latitude > 90.0 {
        return Err(ValidationError::InvalidPostData(
            "Invalid latitude range".to_string()
        ));
    }
    
    if post.longitude < -180.0 || post.longitude > 180.0 {
        return Err(ValidationError::InvalidPostData(
            "Invalid longitude range".to_string()
        ));
    }
    
   
    let now = chrono::Utc::now();
    let future_tolerance = chrono::Duration::minutes(5);
    if post.date > now + future_tolerance {
        return Err(ValidationError::InvalidPostData(
            "Post date cannot be in the future".to_string()
        ));
    }
    
    Ok(())
}
