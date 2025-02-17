# `0xF0` - TWisp
This extension adds support for creating and forwarding PTY terminals through Wisp. The terminals act as streams with flow control, i.e. TCP streams.

Streams created with the stream type `0x03` will ignore the requested port and treat the requested hostname as a command to run in a new PTY, so creating a stream with the destination hostname `/bin/bash` will run `/bin/bash` in a new PTY and forward input/output through the Wisp stream.

## Server Message
Empty

## Client Message
Empty

## Additional Packets
### `0xF0` - TWisp Resize
| Field Name | Field Type | Notes                                            |
|------------|------------|--------------------------------------------------|
| Rows       | `uint16_t` | The number of rows to resize the terminal to.    |
| Columns    | `uint16_t` | The number of columns to resize the terminal to. |

This packet will resize the PTY for the stream ID it is sent with.
