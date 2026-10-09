/* A generic Cortex-M part, large enough for any probe build: the size gate, not the
   linker, is what bounds the image. */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 2M
  RAM   : ORIGIN = 0x20000000, LENGTH = 512K
}
