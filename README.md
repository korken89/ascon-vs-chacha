# Ascon vs Chacha{8,12,20}Poly1305

In this repo the testing code for encrytion/decryption using Ascon and Chacha is compared.

Result:

```
Ascon128 encrypt
    Runtime (16 bytes)  =  9094 cycles/142 us, 568 cycles/byte, 8 us/byte
    Runtime (32 bytes)  = 11892 cycles/185 us, 371 cycles/byte, 5 us/byte
    Runtime (64 bytes)  = 17464 cycles/272 us, 272 cycles/byte, 4 us/byte
    Runtime (128 bytes) = 28673 cycles/448 us, 224 cycles/byte, 3 us/byte

Ascon128 decrypt
    Runtime (16 bytes)  =  9328 cycles/145 us, 583 cycles/byte, 9 us/byte
    Runtime (32 bytes)  = 12126 cycles/189 us, 378 cycles/byte, 5 us/byte
    Runtime (64 bytes)  = 17729 cycles/277 us, 277 cycles/byte, 4 us/byte
    Runtime (128 bytes) = 28933 cycles/452 us, 226 cycles/byte, 3 us/byte

Chacha8 encrypt
    Runtime (16 bytes)  =  7245 cycles/113 us, 452 cycles/byte, 7 us/byte
    Runtime (32 bytes)  =  7655 cycles/119 us, 239 cycles/byte, 3 us/byte
    Runtime (64 bytes)  =  9245 cycles/144 us, 144 cycles/byte, 2 us/byte
    Runtime (128 bytes) = 13075 cycles/204 us, 102 cycles/byte, 1 us/byte

Chacha8 decrypt
    Runtime (16 bytes)  =  7230 cycles/112 us, 451 cycles/byte, 7 us/byte
    Runtime (32 bytes)  =  7672 cycles/119 us, 239 cycles/byte, 3 us/byte
    Runtime (64 bytes)  =  9414 cycles/147 us, 147 cycles/byte, 2 us/byte
    Runtime (128 bytes) = 13169 cycles/205 us, 102 cycles/byte, 1 us/byte

Chacha12 encrypt
    Runtime (16 bytes)  =  7516 cycles/117 us, 469 cycles/byte, 7 us/byte
    Runtime (32 bytes)  =  7975 cycles/124 us, 249 cycles/byte, 3 us/byte
    Runtime (64 bytes)  =  9511 cycles/148 us, 148 cycles/byte, 2 us/byte
    Runtime (128 bytes) = 13365 cycles/208 us, 104 cycles/byte, 1 us/byte

Chacha12 decrypt
    Runtime (16 bytes)  =  7687 cycles/120 us, 480 cycles/byte, 7 us/byte
    Runtime (32 bytes)  =  8053 cycles/125 us, 251 cycles/byte, 3 us/byte
    Runtime (64 bytes)  =  9675 cycles/151 us, 151 cycles/byte, 2 us/byte
    Runtime (128 bytes) = 13503 cycles/210 us, 105 cycles/byte, 1 us/byte

Chacha20 encrypt
    Runtime (16 bytes)  =  8919 cycles/139 us, 557 cycles/byte, 8 us/byte
    Runtime (32 bytes)  =  9401 cycles/146 us, 293 cycles/byte, 4 us/byte
    Runtime (64 bytes)  = 10950 cycles/171 us, 171 cycles/byte, 2 us/byte
    Runtime (128 bytes) = 15529 cycles/242 us, 121 cycles/byte, 1 us/byte

Chacha20 decrypt
    Runtime (16 bytes)  =  9106 cycles/142 us, 569 cycles/byte, 8 us/byte
    Runtime (32 bytes)  =  9474 cycles/148 us, 296 cycles/byte, 4 us/byte
    Runtime (64 bytes)  = 11049 cycles/172 us, 172 cycles/byte, 2 us/byte
    Runtime (128 bytes) = 15659 cycles/244 us, 122 cycles/byte, 1 us/byte
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
licensed as above, without any additional terms or conditions.

[Knurling]: https://knurling.ferrous-systems.com
[Ferrous Systems]: https://ferrous-systems.com/
[GitHub Sponsors]: https://github.com/sponsors/knurling-rs
