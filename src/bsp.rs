use crate::{
    bsp::{ascon::*, chacha::*},
    exit,
};
use embassy_nrf::{
    bind_interrupts, config,
    pac::DWT,
    peripherals,
    rng::{self, Rng},
};
use heapless::Vec;

mod ascon {
    use ascon_aead::aead::{AeadMutInPlace, KeyInit};
    use ascon_aead::{Ascon128, Ascon128Key, Ascon128Nonce};
    use heapless::Vec;

    #[inline(never)]
    pub fn test_ascon_encrypt<const N: usize>(
        key: &Ascon128Key,
        nonce: &Ascon128Nonce,
        inout: &mut Vec<u8, N>,
    ) {
        let mut cipher = Ascon128::new(key);

        // Generates ciphertext + tag in output buffer
        cipher
            .encrypt_in_place(nonce, b"", inout)
            .expect("encryption failure!");
    }

    #[inline(never)]
    pub fn test_ascon_decrypt<const N: usize>(
        key: &Ascon128Key,
        nonce: &Ascon128Nonce,
        inout: &mut Vec<u8, N>,
    ) {
        let mut cipher = Ascon128::new(key);

        // Generates plaintext in output buffer
        cipher
            .decrypt_in_place(nonce, b"", inout)
            .expect("decryption failure!");
    }
}

mod chacha {
    use chacha20poly1305::{
        aead::{heapless::Vec, AeadMutInPlace, KeyInit},
        ChaCha12Poly1305, ChaCha20Poly1305, ChaCha8Poly1305, Key, Nonce,
    };

    #[inline(never)]
    pub fn test_chacha20_encrypt<const N: usize>(key: &Key, nonce: &Nonce, inout: &mut Vec<u8, N>) {
        let mut cipher = ChaCha20Poly1305::new(key);

        // Generates ciphertext + tag in output buffer
        cipher
            .encrypt_in_place(nonce, b"", inout)
            .expect("encryption failure!");
    }

    #[inline(never)]
    pub fn test_chacha20_decrypt<const N: usize>(key: &Key, nonce: &Nonce, inout: &mut Vec<u8, N>) {
        let mut cipher = ChaCha20Poly1305::new(key);

        // Generates plaintext in output buffer
        cipher
            .decrypt_in_place(nonce, b"", inout)
            .expect("decryption failure!");
    }

    #[inline(never)]
    pub fn test_chacha12_encrypt<const N: usize>(key: &Key, nonce: &Nonce, inout: &mut Vec<u8, N>) {
        let mut cipher = ChaCha12Poly1305::new(key);

        // Generates ciphertext + tag in output buffer
        cipher
            .encrypt_in_place(nonce, b"", inout)
            .expect("encryption failure!");
    }

    #[inline(never)]
    pub fn test_chacha12_decrypt<const N: usize>(key: &Key, nonce: &Nonce, inout: &mut Vec<u8, N>) {
        let mut cipher = ChaCha12Poly1305::new(key);

        // Generates plaintext in output buffer
        cipher
            .decrypt_in_place(nonce, b"", inout)
            .expect("decryption failure!");
    }

    #[inline(never)]
    pub fn test_chacha8_encrypt<const N: usize>(key: &Key, nonce: &Nonce, inout: &mut Vec<u8, N>) {
        let mut cipher = ChaCha8Poly1305::new(key);

        // Generates ciphertext + tag in output buffer
        cipher
            .encrypt_in_place(nonce, b"", inout)
            .expect("encryption failure!");
    }

    #[inline(never)]
    pub fn test_chacha8_decrypt<const N: usize>(key: &Key, nonce: &Nonce, inout: &mut Vec<u8, N>) {
        let mut cipher = ChaCha8Poly1305::new(key);

        // Generates plaintext in output buffer
        cipher
            .decrypt_in_place(nonce, b"", inout)
            .expect("decryption failure!");
    }
}

bind_interrupts!(struct Irqs {
    RNG => rng::InterruptHandler<peripherals::RNG>;
});

macro_rules! bench {
    ($body:block, $bytes:literal) => {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        let start = DWT::cycle_count();
        $body
        let diff = DWT::cycle_count() - start;
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        defmt::println!("    Runtime ({} bytes) = {} cycles/{} us, {} cycles/byte, {} us/byte",
            $bytes, diff, diff / 64, diff / $bytes, diff / $bytes / 64);
    };
}

/// Create test data.
fn data(
    rng: &mut Rng<'static, peripherals::RNG>,
) -> (Vec<u8, 32>, Vec<u8, 48>, Vec<u8, 80>, Vec<u8, 144>) {
    let data_16 = {
        let mut data = [0; 16];
        rng.blocking_fill_bytes(&mut data);
        Vec::<u8, { 16 + 16 }>::from_slice(&data).unwrap()
    };
    let data_32 = {
        let mut data = [0; 32];
        rng.blocking_fill_bytes(&mut data);
        Vec::<u8, { 32 + 16 }>::from_slice(&data).unwrap()
    };
    let data_64 = {
        let mut data = [0; 64];
        rng.blocking_fill_bytes(&mut data);
        Vec::<u8, { 64 + 16 }>::from_slice(&data).unwrap()
    };
    let data_128 = {
        let mut data = [0; 128];
        rng.blocking_fill_bytes(&mut data);
        Vec::<u8, { 128 + 16 }>::from_slice(&data).unwrap()
    };

    (data_16, data_32, data_64, data_128)
}

#[inline(always)]
pub fn init(mut c: cortex_m::Peripherals) {
    defmt::info!("Dongle BSP init");

    c.DCB.enable_trace();
    c.DWT.enable_cycle_counter();

    let config = config::Config::default();
    let p = embassy_nrf::init(config);
    let mut rng = Rng::new(p.RNG, Irqs);

    // Ascon key and nonce.
    let mut key = [0; 16];
    rng.blocking_fill_bytes(&mut key);

    let mut nonce = [0; 16];
    rng.blocking_fill_bytes(&mut nonce);

    let (mut data_16, mut data_32, mut data_64, mut data_128) = data(&mut rng);
    defmt::println!("");
    defmt::println!("Ascon128 encrypt");
    bench!(
        { test_ascon_encrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_ascon_encrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_ascon_encrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_ascon_encrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    defmt::println!("");
    defmt::println!("Ascon128 decrypt");
    bench!(
        { test_ascon_decrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_ascon_decrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_ascon_decrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_ascon_decrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    // Chacha key and nonce.
    let mut key = [0; 32];
    rng.blocking_fill_bytes(&mut key);

    let mut nonce = [0; 12];
    rng.blocking_fill_bytes(&mut nonce);

    let (mut data_16, mut data_32, mut data_64, mut data_128) = data(&mut rng);
    defmt::println!("");
    defmt::println!("Chacha8 encrypt");
    bench!(
        { test_chacha8_encrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_chacha8_encrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_chacha8_encrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_chacha8_encrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    defmt::println!("");
    defmt::println!("Chacha8 decrypt");
    bench!(
        { test_chacha8_decrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_chacha8_decrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_chacha8_decrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_chacha8_decrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    let (mut data_16, mut data_32, mut data_64, mut data_128) = data(&mut rng);
    defmt::println!("");
    defmt::println!("Chacha12 encrypt");
    bench!(
        { test_chacha12_encrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_chacha12_encrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_chacha12_encrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_chacha12_encrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    defmt::println!("");
    defmt::println!("Chacha12 decrypt");
    bench!(
        { test_chacha12_decrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_chacha12_decrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_chacha12_decrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_chacha12_decrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    let (mut data_16, mut data_32, mut data_64, mut data_128) = data(&mut rng);
    defmt::println!("");
    defmt::println!("Chacha20 encrypt");
    bench!(
        { test_chacha20_encrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_chacha20_encrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_chacha20_encrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_chacha20_encrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    defmt::println!("");
    defmt::println!("Chacha20 decrypt");
    bench!(
        { test_chacha20_decrypt(&key.into(), &nonce.into(), &mut data_16) },
        16
    );
    bench!(
        { test_chacha20_decrypt(&key.into(), &nonce.into(), &mut data_32) },
        32
    );
    bench!(
        { test_chacha20_decrypt(&key.into(), &nonce.into(), &mut data_64) },
        64
    );
    bench!(
        { test_chacha20_decrypt(&key.into(), &nonce.into(), &mut data_128) },
        128
    );

    exit();
}
