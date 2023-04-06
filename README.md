# lessanvil
Small CLI application to reduce a Minecraft: Java Edition's world size by deleting unused chunks.

This works by looking at the InhabitedTime NBT-tag (see [here](https://minecraft.fandom.com/wiki/Chunk_format) for more information) of each chunk and deleting it in case it's lower than specified value.

# Usage
See `lessanvil --help`

# Installation

## Using Cargo
If you have [cargo](https://github.com/rust-lang/cargo) installed you can use it to compile and install lessanvil.
```
cargo install lessanvil
```

## Get the precompiled binary
Alternatively you can download a precompiled binary from the releases page.
