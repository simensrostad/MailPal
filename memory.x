/* Memory layout for nRF9151 with Embassy modem driver
 *
 * IPC memory must be in lower RAM for modem access.
 * This layout matches the official Embassy nrf9160 modem example.
 */

MEMORY
{
    FLASH : ORIGIN = 0x00000000, LENGTH = 1024K
    IPC   : ORIGIN = 0x20000000, LENGTH = 64K
    RAM   : ORIGIN = 0x20010000, LENGTH = 192K
}

/* Provide IPC region symbols for the modem driver */
PROVIDE(__start_ipc = ORIGIN(IPC));
PROVIDE(__end_ipc   = ORIGIN(IPC) + LENGTH(IPC));
