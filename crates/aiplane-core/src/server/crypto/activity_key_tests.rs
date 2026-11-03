use super::*;

#[test]
fn the_activity_key_is_hmac_sha256_under_a_key_derived_from_the_at_rest_key() {
    let crypto = Crypto::from_key([7u8; 32]);
    let ring = crypto.activity_keys();
    assert_eq!(ring.len(), 1);
    let key = &ring[0];
    assert_eq!(key.id.len(), 16);
    let expected = {
        let mut mac = <Hmac<Sha256>>::new_from_slice(&derive(&[7u8; 32], ACTIVITY_LABEL)).unwrap();
        mac.update(b"event");
        hex_encode(&mac.finalize().into_bytes())
    };
    assert_eq!(key.sign(b"event"), expected);
    assert_ne!(key.sign(b"event"), sha256_hex(b"event"));
    assert_ne!(
        Crypto::from_key([8u8; 32]).activity_keys()[0].sign(b"event"),
        key.sign(b"event"),
        "another gateway's key signs differently"
    );
}
