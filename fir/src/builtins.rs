/// Builtin class/type names used in FIR lowering dispatch.
pub mod class {
    pub const MUTEX: &str = "Mutex";
    pub const CHANNEL: &str = "Channel";
    pub const MULTI_SEND: &str = "MultiSend";
    pub const MULTI_RECEIVE: &str = "MultiReceive";
    pub const FILE: &str = "File";
    pub const RANGE: &str = "Range";
    pub const ITERATOR: &str = "Iterator";
    pub const ORDERING: &str = "Ordering";
}

/// Builtin method names used in FIR lowering dispatch.
pub mod method {
    // List / collection methods
    pub const LEN: &str = "len";
    pub const PUSH: &str = "push";
    pub const RANDOM: &str = "random";

    // Iterable vocabulary
    pub const MAP: &str = "map";
    pub const FILTER: &str = "filter";
    pub const FIND: &str = "find";
    pub const ANY: &str = "any";
    pub const ALL: &str = "all";
    pub const REDUCE: &str = "reduce";
    pub const FIRST: &str = "first";
    pub const LAST: &str = "last";
    pub const COUNT: &str = "count";
    pub const MIN: &str = "min";
    pub const MAX: &str = "max";
    pub const SORT: &str = "sort";
    pub const TO_LIST: &str = "to_list";

    // Protocol methods
    pub const EQ: &str = "eq";
    pub const CMP: &str = "cmp";
    pub const TO_STRING: &str = "to_string";
    pub const DEBUG: &str = "debug";
    pub const EACH: &str = "each";

    // Mutex methods
    pub const ACQUIRE: &str = "acquire";
    pub const RELEASE: &str = "release";
    pub const LOCK: &str = "lock";

    // Channel methods
    pub const SEND: &str = "send";
    pub const WAIT_SEND: &str = "wait_send";
    pub const TRY_SEND: &str = "try_send";
    pub const RECEIVE: &str = "receive";
    pub const WAIT_RECEIVE: &str = "wait_receive";
    pub const TRY_RECEIVE: &str = "try_receive";
    pub const CLOSE: &str = "close";
    pub const CLONE_SENDER: &str = "clone_sender";
    pub const CLONE_RECEIVER: &str = "clone_receiver";

    // File static methods
    pub const READ: &str = "read";
    pub const WRITE: &str = "write";
    pub const APPEND: &str = "append";

    // Nullable / error handling
    pub const OR: &str = "or";
    pub const OR_ELSE: &str = "or_else";
    pub const OR_THROW: &str = "or_throw";

    // Task methods
    pub const IS_READY: &str = "is_ready";
    pub const CANCEL: &str = "cancel";
    pub const WAIT_CANCEL: &str = "wait_cancel";

    // Drop/Close traits
    pub const DROP: &str = "drop";
}
