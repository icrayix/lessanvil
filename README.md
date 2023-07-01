# lessanvil

## !IMPORTANT! For the CLI see [here](cli/README.md)

## About

Lessanvil is a rust library to reduce a Minecraft: Java Edition's world size by deleting unused chunks.
This works by looking at the InhabitedTime NBT-tag (see [here](https://minecraft.fandom.com/wiki/Chunk_format) for more information) of each chunk and deleting the chunk in case it's lower than user-specified value.

The docs are available [here](https://docs.rs/lessanvil)

## CLI

There's a offical CLI and docker image available. See [here](cli/README.md) for more information.
