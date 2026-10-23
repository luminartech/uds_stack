/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/**
 * @file uds_c.h
 * @brief The C side of a uds_stack UDS server: the tables and handlers C
 *        firmware provides, the record copy it answers through, and the
 *        socket and clock calls it implements.
 *
 * The server owns every request and response buffer. A handler reads its
 * request through const pointers and answers through uds_record_copy; it
 * never writes server memory. Every call runs in one execution context, the
 * one that polls the server; none is ISR-safe.
 */
#ifndef UDS_C_H
#define UDS_C_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Negative response codes (ISO 14229-1:2020 Table A.1). */

/**
 * @brief A handler's result: 0x00 is the standard's positiveResponse row,
 *        never a negative code on the wire, so it is the success value.
 */
typedef uint8_t UdsNrc;

#define UDS_NRC_POSITIVE_RESPONSE ((UdsNrc) 0x00U)
#define UDS_NRC_GENERAL_REJECT ((UdsNrc) 0x10U)
#define UDS_NRC_SERVICE_NOT_SUPPORTED ((UdsNrc) 0x11U)
#define UDS_NRC_SUBFUNCTION_NOT_SUPPORTED ((UdsNrc) 0x12U)
#define UDS_NRC_INCORRECT_LENGTH_OR_FORMAT ((UdsNrc) 0x13U)
#define UDS_NRC_RESPONSE_TOO_LONG ((UdsNrc) 0x14U)
#define UDS_NRC_BUSY_REPEAT_REQUEST ((UdsNrc) 0x21U)
#define UDS_NRC_CONDITIONS_NOT_CORRECT ((UdsNrc) 0x22U)
#define UDS_NRC_REQUEST_SEQUENCE_ERROR ((UdsNrc) 0x24U)
#define UDS_NRC_REQUEST_OUT_OF_RANGE ((UdsNrc) 0x31U)
#define UDS_NRC_SECURITY_ACCESS_DENIED ((UdsNrc) 0x33U)
#define UDS_NRC_INVALID_KEY ((UdsNrc) 0x35U)
#define UDS_NRC_EXCEEDED_NUMBER_OF_ATTEMPTS ((UdsNrc) 0x36U)
#define UDS_NRC_REQUIRED_TIME_DELAY_NOT_EXPIRED ((UdsNrc) 0x37U)
#define UDS_NRC_UPLOAD_DOWNLOAD_NOT_ACCEPTED ((UdsNrc) 0x70U)
#define UDS_NRC_TRANSFER_DATA_SUSPENDED ((UdsNrc) 0x71U)
#define UDS_NRC_GENERAL_PROGRAMMING_FAILURE ((UdsNrc) 0x72U)
#define UDS_NRC_WRONG_BLOCK_SEQUENCE_COUNTER ((UdsNrc) 0x73U)
#define UDS_NRC_RESPONSE_PENDING ((UdsNrc) 0x78U)
#define UDS_NRC_SUBFUNCTION_NOT_SUPPORTED_IN_ACTIVE_SESSION ((UdsNrc) 0x7EU)
#define UDS_NRC_SERVICE_NOT_SUPPORTED_IN_ACTIVE_SESSION ((UdsNrc) 0x7FU)

/* Operation status */

/**
 * @brief How the server calls a handler that may take time: INITIAL first;
 *        PENDING on each later poll while it returns UDS_NRC_RESPONSE_PENDING
 *        (the server sends the 0x78s); CANCEL once if the request is
 *        abandoned meanwhile, whose result is ignored.
 */
typedef uint8_t UdsOpStatus;

#define UDS_OP_INITIAL ((UdsOpStatus) 0x00U)
#define UDS_OP_PENDING ((UdsOpStatus) 0x01U)
#define UDS_OP_CANCEL ((UdsOpStatus) 0x02U)

/* Sessions and access */

/* Session bits of an access mask; not the 0x10 sub-function values. */
#define UDS_SESSION_MASK_DEFAULT (0x01U)
#define UDS_SESSION_MASK_PROGRAMMING (0x02U)
#define UDS_SESSION_MASK_EXTENDED (0x04U)
#define UDS_SESSION_MASK_ALL (0x07U)

/* DiagnosticSessionControl sub-functions (ISO 14229-1 Table 25). */
#define UDS_DSC_DEFAULT_SESSION (0x01U)
#define UDS_DSC_PROGRAMMING_SESSION (0x02U)
#define UDS_DSC_EXTENDED_SESSION (0x03U)

/* EcuReset sub-function hardReset (ISO 14229-1 Table 34). */
#define UDS_ECU_RESET_HARD (0x01U)

/**
 * @brief The sessions (UDS_SESSION_MASK_* bits) an operation is available
 *        in, and the security level (its requestSeed value) that must be
 *        unlocked for it, 0 for none.
 */
typedef struct
{
    uint8_t sessions;
    uint8_t security_level;
} UdsAccess;

/* Records: the data a handler answers with */

/**
 * @brief Copies `len` bytes from `src` into the record of the request being
 *        served, at byte `offset`. The server owns the record and bounds it
 *        by the identifier's table entry (a DID's `len`, a routine's
 *        `status_len`, a level's `seed_len`); what a pending handler copied
 *        stays until the request completes. Valid only inside a handler that
 *        answers with a record.
 * @return true once copied; false, copying nothing, outside such a handler,
 *         for a NULL `src`, or for a copy past the bound.
 */
bool uds_record_copy(size_t offset, uint8_t const *src, size_t len);

/* ReadDataByIdentifier (0x22) and WriteDataByIdentifier (0x2E) */

/**
 * @brief Serves one DID: copies its record (the entry's `len` bytes, zero
 *        where left unfilled) and returns UDS_NRC_POSITIVE_RESPONSE,
 *        UDS_NRC_RESPONSE_PENDING, or the NRC to answer.
 */
typedef UdsNrc (*UdsDidReadFn)(UdsOpStatus op);

/** @brief Stores one DID's record, exactly the entry's `len` bytes. */
typedef UdsNrc (*UdsDidWriteFn)(UdsOpStatus op, uint8_t const *data, size_t len);

/** @brief One DID. */
typedef struct
{
    uint16_t      did;
    uint8_t       read_sessions; /* UDS_SESSION_MASK_* bits */
    UdsDidReadFn  read;          /* NULL: not readable */
    UdsAccess     write_access;
    uint16_t      len;   /* the record a read answers and a write carries */
    UdsDidWriteFn write; /* NULL: not writable */
} UdsDidEntry;

/* RoutineControl (0x31) */

/**
 * @brief Starts, stops or reports a routine with its option record; copies
 *        its status record, up to the entry's `status_len` bytes, of which
 *        the response carries as many as were copied.
 */
typedef UdsNrc (*UdsRoutineFn)(UdsOpStatus op, uint8_t const *option, size_t option_len);

/** @brief One routine; a NULL step is not supported. */
typedef struct
{
    uint16_t     rid;
    uint16_t     status_len; /* the longest status record a step answers */
    UdsAccess    access;
    UdsRoutineFn start;
    UdsRoutineFn stop;
    UdsRoutineFn results;
} UdsRidEntry;

/* SecurityAccess (0x27) */

/** @brief Copies the seed, the level's `seed_len` bytes, for a requestSeed. */
typedef UdsNrc (*UdsSeedFn)(UdsOpStatus op, uint8_t const *option, size_t option_len);

/**
 * @brief Judges a sendKey: UDS_NRC_POSITIVE_RESPONSE for the right key,
 *        UDS_NRC_INVALID_KEY for a wrong one (counted towards the level's
 *        delay), UDS_NRC_RESPONSE_PENDING, or another NRC refusing it.
 */
typedef UdsNrc (*UdsKeyFn)(UdsOpStatus op, uint8_t const *key, size_t key_len);

/** @brief One security level. */
typedef struct
{
    uint8_t   level;         /* requestSeed sub-function, odd */
    uint8_t   sessions;      /* UDS_SESSION_MASK_* bits */
    uint8_t   attempt_limit; /* failed keys before the delay */
    uint8_t   seed_len;      /* the seed's length */
    uint32_t  delay_ms;      /* lockout after attempt_limit failures */
    UdsSeedFn seed;
    UdsKeyFn  key;
} UdsSecurityLevel;

/* ReadDTCInformation (0x19), ClearDiagnosticInformation (0x14), ControlDTCSetting (0x85) */

/**
 * @brief Copies a 0x19 response after its sub-function byte for
 *        `report_type` (0x01, 0x02, 0x03, 0x04 or 0x0A) and the parameters it
 *        has, 0 where it has none: `status_mask` (0x01, 0x02), `dtc` and
 *        `record_number` (0x04); the response carries as many bytes as were
 *        copied, at most a report of every DTC the policy's MAX_DTCS allows.
 */
typedef UdsNrc (*UdsDtcReportFn)(UdsOpStatus op, uint8_t report_type, uint8_t status_mask, uint32_t dtc, uint8_t record_number);

/**
 * @brief Clears the DTCs of `group` (0xFFFFFF: all). A request that carries a
 *        memorySelection is answered requestOutOfRange without this being called.
 */
typedef UdsNrc (*UdsDtcClearFn)(UdsOpStatus op, uint32_t group);

/** @brief Turns DTC status updates on (0x01) or off (0x02). */
typedef UdsNrc (*UdsDtcSettingFn)(UdsOpStatus op, uint8_t setting_type);

/** @brief A layer's DTC handlers; NULL: not served by this layer. */
typedef struct
{
    UdsDtcReportFn  report;
    UdsDtcClearFn   clear;
    UdsDtcSettingFn setting;
} UdsDtcHooks;

/* CommunicationControl (0x28) */

/** @brief Enables or disables reception and transmission of `communication_type`. */
typedef UdsNrc (*UdsCommControlFn)(UdsOpStatus op, uint8_t control_type, uint8_t communication_type);

/* RequestDownload (0x34), TransferData (0x36), RequestTransferExit (0x37) */

/** @brief Accepts a download of `size` bytes to `address` in `data_format`. */
typedef UdsNrc (*UdsTransferBeginFn)(UdsOpStatus op, uint32_t address, uint32_t size, uint8_t data_format);

/** @brief Stores the next block of the download. */
typedef UdsNrc (*UdsTransferBlockFn)(UdsOpStatus op, uint8_t const *data, size_t len);

/** @brief Completes the download; called only after an accepted begin. */
typedef UdsNrc (*UdsTransferFinishFn)(UdsOpStatus op);

/** @brief A layer's download handlers; NULL: not served by this layer. */
typedef struct
{
    UdsAccess           access;
    UdsTransferBeginFn  begin;
    UdsTransferBlockFn  block;
    UdsTransferFinishFn finish;
} UdsTransferHooks;

/* EcuReset (0x11) and DiagnosticSessionControl (0x10) */

/**
 * @brief Whether the transition to `sub_function` may be made: 0x00 or the NRC
 *        refusing it, at once. UDS_NRC_RESPONSE_PENDING is answered generalReject.
 */
typedef UdsNrc (*UdsTransitionCheckFn)(uint8_t sub_function);

/**
 * @brief Brings the system to where the transition can be made: 0x00 once
 *        done, UDS_NRC_RESPONSE_PENDING meanwhile, or the NRC refusing it.
 *        Nothing undoes a prepare: a later layer may refuse the transition, or
 *        the request be canceled, so a prepared system is one that is also
 *        valid without the transition.
 */
typedef UdsNrc (*UdsTransitionPrepareFn)(UdsOpStatus op, uint8_t sub_function);

/**
 * @brief Makes the transition, once its positive response has gone out. A session's
 *        also runs, with UDS_DSC_DEFAULT_SESSION, when the server returns to the
 *        default session without a request: on tS3_Server expiry, or after a reset.
 */
typedef void (*UdsTransitionExecuteFn)(uint8_t sub_function);

/**
 * @brief A layer's say in one service's transitions; any hook may be NULL.
 *        The server asks every layer's check, then every layer's prepare,
 *        sends the positive response, then runs every layer's execute, the
 *        last layer's first.
 */
typedef struct
{
    UdsTransitionCheckFn   check;
    UdsTransitionPrepareFn prepare;
    UdsTransitionExecuteFn execute;
} UdsTransitionHooks;

/* Layers */

/**
 * @brief One layer of services: a table per identifier-based service and
 *        handlers for the others, NULL or empty where the layer serves none.
 *        The server searches its layers in the order given, so an earlier
 *        layer's entry wins; a later layer adds services but never replaces
 *        one. A DTC, communication control or download service that no layer
 *        serves is answered serviceNotSupported.
 */
typedef struct
{
    UdsDidEntry const      *dids;
    size_t                  did_count;
    UdsRidEntry const      *rids;
    size_t                  rid_count;
    UdsSecurityLevel const *security_levels;
    size_t                  security_level_count;
    UdsDtcHooks             dtc;
    UdsCommControlFn        comm_control;
    UdsTransferHooks        transfer;
    UdsTransitionHooks      session; /* DiagnosticSessionControl (0x10) */
    UdsTransitionHooks      reset;   /* EcuReset (0x11) */
} UdsLayer;

/* Sockets and clock, implemented by the firmware */

/** Socket call result: nothing to do yet; the server calls again later. */
#define UDS_SOCK_WOULD_BLOCK ((int32_t) -1)
/** Socket call result: the socket is gone or the stack failed. */
#define UDS_SOCK_ERR ((int32_t) -2)

/* A call that returns UDS_SOCK_WOULD_BLOCK has changed nothing, so the server
 * may abandon it and call again. uds_udp_send sends or fails at once, so the
 * server takes any negative result from it, UDS_SOCK_WOULD_BLOCK included, as
 * a failure. `sock` is the index uds_tcp_accept handed out, valid until
 * uds_tcp_release. Addresses are IPv4, host byte order. */

/**
 * @brief Hands out an accepted connection.
 * @param[out] peer_addr The peer's address, written on success.
 * @param[out] peer_port The peer's port, written on success.
 * @return The new connection's socket index, 0 or more; UDS_SOCK_WOULD_BLOCK
 *         while none is waiting; UDS_SOCK_ERR if the listener failed.
 */
int32_t uds_tcp_accept(uint32_t *peer_addr, uint16_t *peer_port);

/**
 * @brief Whether a read of `sock` would complete now.
 * @param sock The connection's socket index.
 * @return true where data, the end of the stream or an error is waiting.
 */
bool uds_tcp_readable(int32_t sock);

/**
 * @brief Copies received bytes out.
 * @param sock The connection's socket index.
 * @param[out] buf Where the bytes go; `cap` bytes are writable.
 * @param cap The most bytes to copy.
 * @return The count copied, at most `cap`; 0 at the end of the stream;
 *         UDS_SOCK_WOULD_BLOCK while nothing is waiting; or UDS_SOCK_ERR.
 */
int32_t uds_tcp_read(int32_t sock, uint8_t *buf, size_t cap);

/**
 * @brief Queues bytes to send.
 * @param sock The connection's socket index.
 * @param buf The bytes; `len` bytes are readable, and are copied before return.
 * @param len How many bytes to send.
 * @return The count taken, at most `len` (a partial write takes fewer, 0 for a
 *         `len` of 0); UDS_SOCK_WOULD_BLOCK while none can be taken; or
 *         UDS_SOCK_ERR.
 */
int32_t uds_tcp_write(int32_t sock, uint8_t const *buf, size_t len);

/**
 * @brief Closes the connection. Idempotent.
 * @param sock The connection's socket index.
 * @param reset true to reset the connection, false to close it gracefully.
 */
void uds_tcp_close(int32_t sock, bool reset);

/**
 * @brief The server dropped the socket: resets it if still open and frees the
 *        index, which uds_tcp_accept may hand out again.
 * @param sock The connection's socket index.
 */
void uds_tcp_release(int32_t sock);

/**
 * @brief Whether a datagram is queued.
 * @return true where uds_udp_recv would return one.
 */
bool uds_udp_readable(void);

/**
 * @brief Takes the oldest queued datagram.
 * @param[out] buf Where the datagram goes; `cap` bytes are writable.
 * @param cap The most bytes to copy; a longer datagram is truncated to it.
 * @param[out] src_addr The sender's address, written on success.
 * @param[out] src_port The sender's port, written on success.
 * @return The length copied, at most `cap`; UDS_SOCK_WOULD_BLOCK while none
 *         is queued; or UDS_SOCK_ERR.
 */
int32_t uds_udp_recv(uint8_t *buf, size_t cap, uint32_t *src_addr, uint16_t *src_port);

/**
 * @brief Sends one datagram, at once.
 * @param dst_addr The destination address.
 * @param dst_port The destination port.
 * @param buf The datagram; `len` bytes are readable, and are copied before return.
 * @param len The datagram's length.
 * @return 0 once sent; UDS_SOCK_ERR if it cannot be.
 */
int32_t uds_udp_send(uint32_t dst_addr, uint16_t dst_port, uint8_t const *buf, size_t len);

/**
 * @brief The time, called from the server's context only.
 * @return Monotonic milliseconds, which may wrap.
 */
uint32_t uds_now_ms(void);

/* Layout checks. The uds_c crate reads these structures with the same sizes and
 * offsets, and asserts them for a 32-bit target as well. */

#ifdef __cplusplus
#define UDS_C_STATIC_ASSERT(condition, message) static_assert(condition, message)
#else
#define UDS_C_STATIC_ASSERT(condition, message) _Static_assert(condition, message)
#endif

#if UINTPTR_MAX == 0xFFFFFFFFU
UDS_C_STATIC_ASSERT(sizeof(UdsAccess) == 2U, "UdsAccess layout");
UDS_C_STATIC_ASSERT(sizeof(UdsDidEntry) == 16U, "UdsDidEntry layout");
UDS_C_STATIC_ASSERT(offsetof(UdsDidEntry, len) == 10U, "UdsDidEntry layout");
UDS_C_STATIC_ASSERT(sizeof(UdsRidEntry) == 20U, "UdsRidEntry layout");
UDS_C_STATIC_ASSERT(offsetof(UdsRidEntry, start) == 8U, "UdsRidEntry layout");
UDS_C_STATIC_ASSERT(sizeof(UdsSecurityLevel) == 16U, "UdsSecurityLevel layout");
UDS_C_STATIC_ASSERT(offsetof(UdsSecurityLevel, delay_ms) == 4U, "UdsSecurityLevel layout");
UDS_C_STATIC_ASSERT(sizeof(UdsDtcHooks) == 12U, "UdsDtcHooks layout");
UDS_C_STATIC_ASSERT(sizeof(UdsTransferHooks) == 16U, "UdsTransferHooks layout");
UDS_C_STATIC_ASSERT(sizeof(UdsTransitionHooks) == 12U, "UdsTransitionHooks layout");
UDS_C_STATIC_ASSERT(sizeof(UdsLayer) == 80U, "UdsLayer layout");
UDS_C_STATIC_ASSERT(offsetof(UdsLayer, dtc) == 24U, "UdsLayer layout");
UDS_C_STATIC_ASSERT(offsetof(UdsLayer, transfer) == 40U, "UdsLayer layout");
UDS_C_STATIC_ASSERT(offsetof(UdsLayer, session) == 56U, "UdsLayer layout");
UDS_C_STATIC_ASSERT(offsetof(UdsLayer, reset) == 68U, "UdsLayer layout");
#endif

#undef UDS_C_STATIC_ASSERT

#ifdef __cplusplus
}
#endif

#endif /* UDS_C_H */
