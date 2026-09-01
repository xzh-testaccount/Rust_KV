# B模块：存储与持久化设计说明

## 1. 文档目的

本文说明B负责人已经完成的存储与持久化模块，包括代码结构、数据结构、对外接口、WAL、CRC32、启动恢复、异常处理和测试。本文可用于团队交接、课程报告和答辩说明。

B模块的目标不是调用现成数据库，而是在Rust中实现一个小型键值存储核心：运行时使用内存提供快速访问，修改操作同步记录到磁盘，服务器重启时再根据磁盘记录恢复内存状态。

## 2. B完成的内容

| 内容 | 实现位置 | 当前状态 |
| --- | --- | --- |
| 内存键值存储 | `src/storage.rs` | 已完成 |
| 新增和覆盖 | `Store::set` | 已完成 |
| 查询 | `Store::get` | 已完成 |
| 删除 | `Store::delete` | 已完成 |
| 键的有序输出 | `Store::keys` | 已完成 |
| 数据量和字节统计 | `Store::stats` | 已完成 |
| 键和值校验 | `validate_key`、`validate_value` | 已完成 |
| 不存在键的明确反馈 | `missing_key` | 已完成 |
| 统一错误类型 | `src/error.rs` | 已完成 |
| WAL追加写入 | `PersistentStore::append_record` | 已完成 |
| 写入后立即刷盘 | `flush`、`sync_data` | 已完成 |
| 启动恢复 | `recover`、`replay_record` | 已完成 |
| 文件截断和格式检测 | `recover` | 已完成 |
| CRC32内容校验 | `encode_entry`、`verify_checksum` | 已完成 |
| 损坏文件保护 | 严格恢复策略 | 已完成 |
| 自动化测试 | `tests/`和模块单元测试 | 已完成 |

## 3. B模块在项目中的位置

```text
客户端
  │ 发送SET、GET、DELETE等请求
  ▼
协议层A
  │ 得到Request并定义Response格式
  ▼
网络服务层C
  │ 调用统一存储接口
  ▼
PersistentStore（B）
  ├── Store：内存数据和CRUD
  └── WAL：磁盘日志、CRC32和启动恢复
```

C不需要理解BTreeMap节点、CRC32计算或WAL解析，只需要调用 `PersistentStore` 的公开方法。B模块负责保证一次修改不会只进入内存而漏写日志。

## 4. 内存存储设计

### 4.1 `Store`和`entries`

内存存储定义为：

```rust
pub struct Store {
    entries: BTreeMap<String, String>,
}
```

`Store`是B提供的内存存储对象，`entries`是它内部真正保存键值数据的字段。一个 `BTreeMap<String, String>` 可以保存很多组键值，不是只能保存一组。

```text
Store
└── entries: BTreeMap<String, String>
    ├── "course" → "Rust"
    ├── "name"   → "Alice"
    └── "score"  → "100"
```

每个键只能对应一个值。再次对同一个键执行 `set` 时，新值会覆盖旧值：

```text
SET course Rust
SET course AdvancedRust

最终结果：course → AdvancedRust
```

如果业务上需要“一门课程对应多个成员”，当前接口应使用多个不同键，例如：

```text
course:rust:member:1 → Alice
course:rust:member:2 → Bob
```

而不是让一个键直接对应多个字符串。

### 4.2 为什么使用`BTreeMap`

`BTreeMap`是有序映射，底层是平衡的多路搜索树。数据会按照键的顺序组织，因此 `keys()` 不需要额外排序就能按字典序返回。

常用操作的时间复杂度为：

| 操作 | 时间复杂度 |
| --- | --- |
| `set` | `O(log n)` |
| `get` | `O(log n)` |
| `delete` | `O(log n)` |
| 按顺序遍历全部键 | `O(n)` |

课程要求包含“有序列出全部键”，所以 `BTreeMap` 比无序的 `HashMap` 更适合当前项目，也能体现数据结构选择与需求之间的关系。

### 4.3 新增和覆盖结果

`set`返回：

```rust
pub enum SetOutcome {
    Created,
    Replaced { previous: String },
}
```

含义如下：

- `Created`：这个键原来不存在，本次是新增。
- `Replaced`：这个键已经存在，本次覆盖旧值，并返回旧值。

网络层可以通过：

```rust
outcome.replaced()
```

生成协议中的：

```json
{"ok":true,"data":{"kind":"set","replaced":true}}
```

### 4.4 CRUD接口

内存层提供以下接口：

```rust
Store::new()
Store::set(key, value)
Store::get(key)
Store::delete(key)
Store::keys()
Store::len()
Store::is_empty()
Store::stats()
```

其中：

- `set`新增或者覆盖键值。
- `get`返回值的借用 `&str`，避免不必要的字符串复制。
- `delete`删除键并返回被删除的旧值。
- `keys`返回已经按照字典序排列的键。
- `len`返回当前键值数量。
- `is_empty`判断当前是否为空。
- `stats`统计键值数量以及键和值占用的UTF-8字节数。

## 5. 输入校验与明确反馈

### 5.1 键的规则

键必须满足：

- 不能为空。
- UTF-8字节数不能超过256。
- 不能包含空白字符。
- 不能包含控制字符。

对应反馈：

| 情况 | 错误码 | 错误信息示例 |
| --- | --- | --- |
| 空键 | `INVALID_KEY` | `键不能为空` |
| 键超过256字节 | `INVALID_KEY` | `键长度为 300 字节，最大允许 256 字节` |
| 键包含空格 | `INVALID_KEY` | `键不能包含空白字符` |
| 键包含不可见控制字符 | `INVALID_KEY` | `键不能包含控制字符` |

长度按UTF-8字节计算，不按字符数计算。一个常用汉字通常占3个UTF-8字节。

### 5.2 值的规则

值必须满足：

- 不能为空。
- UTF-8字节数不能超过16 KiB。
- 不能包含控制字符。
- 可以包含普通空格。

对应反馈：

| 情况 | 错误码 | 错误信息示例 |
| --- | --- | --- |
| 空值 | `INVALID_VALUE` | `值不能为空` |
| 值超过16 KiB | `INVALID_VALUE` | `值长度为 X 字节，最大允许 16384 字节` |
| 值包含控制字符 | `INVALID_VALUE` | `值不能包含控制字符` |

### 5.3 不存在的键

`get`或`delete`遇到不存在的键时，统一返回：

```text
错误码：NOT_FOUND
错误信息：键不存在：具体键名
```

`set`遇到不存在的键不会报错，因为这正是新增操作。

### 5.4 为什么校验函数需要复用

`validate_key`和`validate_value`使用 `pub(crate)`：项目内部模块可以调用，项目外部不能直接调用。正常写入和WAL恢复使用同一套规则：

```text
客户端正常SET ─┐
              ├── validate_key / validate_value
启动重放WAL ──┘
```

这样可以避免正常请求拒绝某个值，但恢复程序却接受同一个值的规则不一致问题。

## 6. 统一错误处理

`src/error.rs`定义了两层错误信息。

### 6.1 `ErrorCode`

`ErrorCode`是稳定的错误分类，适合通过协议发送给客户端，例如：

```text
INVALID_KEY
INVALID_VALUE
NOT_FOUND
STORAGE_ERROR
```

错误码保持稳定，方便客户端程序判断错误类型。

### 6.2 `AppError`

`AppError`保存项目内部的完整错误：

```rust
pub enum AppError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Protocol { code: ErrorCode, message: String },
    Storage { message: String },
    CorruptWal { line: usize, reason: String },
    NotImplemented(&'static str),
}
```

主要含义：

- `Io`：文件打开、写入、刷新或者同步失败。
- `Json`：普通JSON编解码错误。
- `Protocol`：键值校验、键不存在等可返回给客户端的错误。
- `Storage`：存储逻辑错误，例如记录数量溢出。
- `CorruptWal`：WAL文件损坏，保存准确行号和原因。
- `NotImplemented`：开发阶段尚未完成的功能。

`code()`把内部错误映射为对外错误码，`client_message()`生成适合客户端阅读的文字。网络层可以直接调用：

```rust
Response::from_error(&error)
```

### 6.3 `Result<T>`和`?`

项目使用：

```rust
pub type Result<T> = std::result::Result<T, AppError>;
```

所以函数可以简写为：

```rust
pub fn get(&self, key: &str) -> Result<&str>
```

`From<std::io::Error>`和 `From<serde_json::Error>` 让代码可以使用 `?` 自动转换并向上传递错误。

## 7. 持久化存储统一入口

### 7.1 `PersistentStore`

```rust
pub struct PersistentStore {
    store: Store,
    wal: BufWriter<File>,
    wal_path: PathBuf,
    writable: bool,
    wal_records: u64,
    wal_bytes: u64,
}
```

字段含义：

| 字段 | 作用 |
| --- | --- |
| `store` | 保存当前内存键值数据 |
| `wal` | 向WAL文件追加内容的缓冲写入器 |
| `wal_path` | WAL文件路径 |
| `writable` | 当前是否允许继续执行修改操作 |
| `wal_records` | WAL历史修改记录数 |
| `wal_bytes` | WAL文件当前字节数 |

这些字段是私有的，C不能直接修改内部 `Store`，也不能取得内部文件句柄。公开的修改入口只有 `PersistentStore::set` 和 `PersistentStore::delete`，因此正常使用时无法漏写WAL。

### 7.2 对外接口

```rust
PersistentStore::open(path)
PersistentStore::set(key, value)
PersistentStore::get(key)
PersistentStore::delete(key)
PersistentStore::keys()
PersistentStore::len()
PersistentStore::is_empty()
PersistentStore::stats()
PersistentStore::wal_path()
```

服务器必须使用：

```rust
let store = PersistentStore::open(&config.wal_path)?;
```

不能使用普通的：

```rust
let store = Store::new();
```

因为 `Store::new()`只会建立一个空内存表，不打开WAL、不恢复数据，也不会在修改时自动写入磁盘。

## 8. WAL设计

### 8.1 WAL是什么

WAL是Write-Ahead Log，中文通常称为预写日志。它不直接保存一份最终表格，而是按顺序保存每次成功的修改操作。

例如操作过程为：

```text
SET course Rust
SET course AdvancedRust
SET temporary value
DELETE temporary
```

WAL就按相同顺序保存四条记录。服务器重启时，从空 `Store` 开始把四条操作再执行一遍，就能得到退出前的最终状态。

### 8.2 操作类型

```rust
enum WalRecord {
    Set { key: String, value: String },
    Delete { key: String },
}
```

WAL只记录 `set` 和 `delete`，因为只有它们会改变数据。`get`、`keys`和 `stats` 不改变最终状态，所以不写WAL。

### 8.3 带CRC32的完整记录

```rust
struct WalEntry {
    record: WalRecord,
    crc32: String,
}
```

实际文件每行类似：

```json
{"record":{"op":"set","key":"course","value":"Rust"},"crc32":"B033579D"}
```

格式规则：

- 文件采用UTF-8 JSON Lines。
- 每一行是一条完整记录。
- 每条记录必须以LF结尾。
- `record`保存操作内容。
- `crc32`是8位十六进制校验和。
- 整行JSON内容不能超过64 KiB，不计算最后的LF。
- 未知字段、未知操作和字段类型错误都不允许。

## 9. CRC32校验原理

### 9.1 写入时计算

`encode_entry`先把内部 `WalRecord` 序列化为稳定的JSON字节，然后调用：

```rust
crc32fast::hash(&record_bytes)
```

得到一个32位数，再格式化成8位大写十六进制字符串：

```rust
format!("{:08X}", checksum)
```

校验和只根据 `record` 的标准JSON字节计算，不把 `crc32` 字段本身包含进去，否则会形成循环依赖。

### 9.2 恢复时验证

`verify_checksum`执行：

```text
读取crc32字符串
→ 检查是否正好8位十六进制
→ 重新序列化record
→ 重新计算CRC32
→ 与文件中的值比较
```

如果有人把：

```json
"value":"Rust"
```

修改成：

```json
"value":"Dust"
```

JSON仍然合法，但重新计算出的CRC32不同，程序会报告：

```text
WAL第 1 行损坏：CRC32校验失败
```

### 9.3 CRC32的边界

CRC32用于发现磁盘位翻转、误修改和传输损坏，不是加密，也不能防止有意伪造。攻击者如果同时修改内容并重新计算CRC32，仍然可以制造一条表面合法的记录。

当前CRC32按记录保护内容，但不能可靠发现整条合法记录被删除或多条合法记录被重新排序。提高项可以加入递增序列号或者前后记录哈希链。

## 10. 修改操作的一致性

### 10.1 `set`流程

```text
1. 检查存储是否可写
2. 校验key和value
3. 构造WalRecord::Set
4. 序列化操作并计算CRC32
5. 追加完整WalEntry和LF
6. flush缓冲区
7. sync_data同步文件
8. 更新内存Store
9. 返回Created或Replaced
```

核心顺序是：

```rust
self.append_record(&record)?;
self.store.set_validated(key, value)
```

如果 `append_record` 失败，`?` 会立即返回，内存更新不会执行。

### 10.2 `delete`流程

```text
1. 检查存储是否可写
2. 校验key
3. 检查key是否存在并复制旧值
4. 构造WalRecord::Delete
5. 计算CRC32并同步写入WAL
6. 从内存Store删除
7. 返回被删除的旧值
```

删除前先检查键是否存在，因此删除不存在的键不会向WAL写入一条无效记录。

### 10.3 为什么必须先写WAL

如果先更新内存再写文件，可能发生：

```text
内存更新成功
→ 服务器向客户端返回成功
→ WAL还没有落盘时断电
→ 重启后数据丢失
```

当前顺序为：

```text
WAL同步成功
→ 更新内存
→ 网络层返回成功
```

因此客户端收到成功时，操作已经具有可恢复记录。

### 10.4 `flush`和`sync_data`

`BufWriter`会先把数据放入用户态缓冲区：

```rust
self.wal.write_all(&encoded)?;
self.wal.write_all(b"\n")?;
self.wal.flush()?;
self.wal.get_ref().sync_data()?;
```

- `write_all`保证尝试写完整内容，而不是只写一部分后当作成功。
- `flush`把 `BufWriter` 中的数据交给操作系统。
- `sync_data`要求操作系统把文件数据同步到底层存储。

只有这几步全部成功，代码才更新内存和WAL统计。

### 10.5 写入失败后的只读保护

文件写入、刷新或同步失败后：

```rust
self.writable = false;
```

后续 `set` 和 `delete` 会被拒绝：

```text
WAL先前写入失败，存储已进入只读状态
```

原因是失败可能在文件末尾留下半条记录。继续追加可能让损坏范围扩大，所以当前实例采用失败即停止写入的策略。读取操作仍然可以继续使用。

## 11. 启动恢复原理

### 11.1 `open`流程

服务器启动时调用：

```rust
PersistentStore::open("data/kv.wal")
```

内部流程：

```text
保存WAL路径
→ 创建缺失的父目录
→ 以append模式创建或打开文件
→ 不截断已有内容
→ 调用recover读取全部记录
→ 获取记录数和文件大小
→ 重新准备追加写入器
→ 返回恢复完成的PersistentStore
```

`append(true)`只向文件末尾添加内容，没有使用 `truncate(true)`，所以重启不会清空历史数据。

### 11.2 `recover`逐行检查

恢复从空内存开始：

```rust
let mut store = Store::new();
```

然后使用 `BufReader::read_until(b'\n', ...)` 逐行读取WAL。每行依次检查：

1. 是否超过64 KiB。
2. 是否以LF结尾。
3. 是否为空或者只有空白字符。
4. 是否是合法JSON。
5. 是否包含规定的 `record` 和 `crc32`。
6. 是否有未知字段或未知操作。
7. CRC32是否为8位十六进制。
8. CRC32是否与重新计算结果一致。
9. 键和值是否仍符合业务规则。
10. `delete`操作恢复到当前步骤时，目标键是否存在。

任意一步失败都会返回带行号的 `CorruptWal`，不会跳过问题记录。

### 11.3 `replay_record`重放

校验通过后，`replay_record`按顺序执行：

```rust
WalRecord::Set { key, value }
    → store.set_validated(key, value)

WalRecord::Delete { key }
    → store.delete_validated(&key)
```

例如WAL依次为：

```text
SET course Rust
SET course AdvancedRust
SET temporary value
DELETE temporary
```

恢复结果为：

```text
course → AdvancedRust
```

`temporary`不存在，因为最后一条记录将它删除了。

### 11.4 严格恢复策略

发现损坏时程序：

- 报告准确行号和原因。
- 拒绝完成 `PersistentStore::open`。
- 不自动跳过记录。
- 不自动截断文件。
- 不自动覆盖或创建一个空数据库代替原数据。

这个策略可以防止程序看似正常启动，但悄悄丢失部分数据。异常文件应由开发人员备份后检查。

## 12. 崩溃场景分析

| 发生时刻 | 结果 |
| --- | --- |
| 参数校验前失败 | WAL和内存都不改变 |
| 写WAL前失败 | WAL和内存都不改变 |
| WAL写入中途失败 | 内存不改变，实例停止写入；重启时检测截断、JSON错误或CRC错误 |
| `sync_data`失败 | 内存不改变，不返回成功，实例停止写入 |
| WAL同步后、内存更新前崩溃 | 重启后根据WAL恢复该操作；客户端可能没有收到成功 |
| 内存更新后、网络响应前崩溃 | 重启后仍能恢复；客户端可能没有收到成功 |
| 客户端收到成功后崩溃 | WAL已经同步，重启后可以恢复 |

设计优先避免“客户端收到成功但重启后数据消失”。在极端崩溃窗口中，可能出现客户端没有收到成功，但操作已经进入WAL并在重启后生效，这比确认成功后丢数据更安全。

## 13. 状态统计

### 13.1 内存统计

```rust
pub struct StoreStats {
    pub entries: usize,
    pub key_bytes: usize,
    pub value_bytes: usize,
}
```

- `entries`：当前键值数量。
- `key_bytes`：所有键占用的UTF-8字节数。
- `value_bytes`：所有值占用的UTF-8字节数。

### 13.2 持久化统计

```rust
pub struct PersistentStats {
    pub store: StoreStats,
    pub wal_records: u64,
    pub wal_bytes: u64,
    pub writable: bool,
}
```

- `store`：内存统计。
- `wal_records`：历史修改记录数，不等于当前键值数。
- `wal_bytes`：WAL文件大小。
- `writable`：是否允许继续修改数据。

例如先新增一个键再删除：

```text
store.entries = 0
wal_records = 2
```

因为最终没有数据，但历史上发生了两次修改。

## 14. 与C的并发集成

C应当共享：

```rust
Arc<Mutex<PersistentStore>>
```

- `Arc`让多个客户端线程持有同一个存储对象。
- `Mutex`保证同一时刻只有一个线程修改内存和WAL。
- 所有修改经过同一个锁，WAL顺序与内存更新顺序保持一致。

锁的正确范围：

```text
读取网络请求       不持锁
解析JSON           不持锁
调用存储接口       持锁
复制需要的查询结果 持锁
构造网络响应       不持锁
发送响应           不持锁
```

`set`和 `delete` 内部的WAL追加、`flush`、`sync_data`和内存修改必须处于同一次锁定中，不能在中间释放锁。

请求映射：

| 请求 | 调用 |
| --- | --- |
| `SET` | `PersistentStore::set` |
| `GET` | `PersistentStore::get` |
| `DELETE` | `PersistentStore::delete` |
| `KEYS` | `PersistentStore::keys` |
| `STATUS` | `PersistentStore::len`或 `stats` |
| `PING` | 不访问存储 |
| `QUIT` | 不访问存储，关闭当前连接 |

## 15. 基本使用示例

```rust
use rust_kv_store::persistence::PersistentStore;

fn example() -> rust_kv_store::error::Result<()> {
    let mut store = PersistentStore::open("data/kv.wal")?;

    let outcome = store.set("course".into(), "Rust".into())?;
    println!("是否覆盖旧值：{}", outcome.replaced());

    let value = store.get("course")?;
    println!("course = {value}");

    println!("有序键列表：{:?}", store.keys());
    println!("当前键值数量：{}", store.len());

    store.delete("course")?;
    Ok(())
}
```

程序退出后再次执行：

```rust
let store = PersistentStore::open("data/kv.wal")?;
```

会重新读取同一WAL并恢复最终状态。

## 16. 测试与验收

### 16.1 内存存储测试

`src/storage.rs`中的单元测试和 `tests/storage_stage3.rs` 验证：

- 新Store为空。
- 新增和覆盖返回正确结果。
- GET和DELETE不存在键时返回 `NOT_FOUND`。
- 删除返回旧值。
- KEYS按字典序输出。
- 状态统计正确。
- 非法键和值不会修改数据。
- UTF-8长度按字节计算。
- 值允许普通空格并能达到最大限制。
- 完整CRUD无需网络即可运行。

### 16.2 持久化测试

`tests/persistence_stage4.rs`包含12个测试函数，覆盖以下场景（部分测试同时检查多个场景）：

- 第一次启动创建目录和空WAL。
- 新增、覆盖和删除可以按顺序恢复。
- 恢复后可以继续追加新记录。
- 成功记录是完整JSON行并包含正确CRC32。
- 非法修改不会改变内存和WAL。
- 截断记录被发现且原文件不被覆盖。
- 非法JSON和未知字段被拒绝。
- 未知操作和非法数据被拒绝。
- 删除不存在键的恢复记录被视为损坏。
- 空记录和超大记录被拒绝。
- 修改合法JSON内容后CRC32校验失败。
- CRC32缺失或者格式错误时拒绝恢复。
- CRC32算法通过标准IEEE测试向量验证。

当前验证命令：

```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
```

最近一次完整测试结果为29项通过、0项失败。

## 17. 已完成标准

B模块现在满足以下基础验收要求：

- 不依赖网络即可完成键值增删改查。
- 键能够有序列出，并能统计当前数据量。
- 不存在键、非法键和值具有明确反馈。
- 所有修改通过统一持久化接口执行。
- 每次成功修改都会及时追加并同步WAL。
- WAL同步完成后才更新内存。
- 使用同一WAL重新启动可以恢复最终状态。
- 能检测文件截断、非法格式、非法业务记录和内容校验失败。
- 问题文件不会被自动忽略或覆盖。

## 18. 当前限制和提高项

以下功能尚未实现，不影响当前基础阶段验收：

1. **Snapshot快照**：当前每次启动都从第一条WAL重放，日志很大时启动会变慢。
2. **WAL压缩与轮换**：历史记录会持续增长，尚未自动清理已被覆盖的旧操作。
3. **格式版本号**：当前新格式严格要求CRC32，不自动兼容旧的无校验和WAL。
4. **操作序列号**：目前不能可靠发现完整合法记录被删除或调换顺序。
5. **跨进程文件锁**：只设计了单服务器进程，多进程同时打开同一WAL不受支持。
6. **事务和批量操作**：一次接口调用只修改一个键。
7. **安全认证**：CRC32不是密码学签名，不能防止有意伪造。

推荐的提高顺序为：

```text
格式版本号和操作序列号
→ Snapshot
→ WAL压缩与轮换
→ 快照原子发布和崩溃恢复
→ 性能指标与压力测试
```

## 19. B与A、C的职责边界

### B交给A

- 稳定错误码和错误信息。
- `SetOutcome::replaced()`用于生成SET响应。
- GET、DELETE、KEYS和STATUS所需的数据结果。
- 键和值的大小限制与校验规则。

### B交给C

- `PersistentStore::open`作为服务器启动入口。
- `Arc<Mutex<PersistentStore>>`作为共享存储类型。
- CRUD、KEYS和STATUS接口。
- `Response::from_error`所需的 `AppError`。
- WAL恢复失败时拒绝启动的约定。

### B不负责

- TCP监听和连接管理。
- 请求帧读取与JSON请求解析。
- 为每个客户端创建线程。
- 网络响应写回和连接关闭。
- 客户端命令行交互。

## 20. 总结

B模块实现了一个“内存索引 + 预写日志”的小型持久化键值存储。`BTreeMap`负责运行时的有序数据访问，WAL负责保存所有成功修改，CRC32负责发现记录内容损坏，启动恢复负责从磁盘重建内存状态，统一错误体系负责把底层异常转换成稳定、明确的反馈。

最关键的设计原则是：

```text
修改数据：校验 → CRC32 → WAL追加 → flush → sync_data → 更新内存

启动恢复：逐行读取 → 格式检查 → CRC32验证 → 业务校验 → 顺序重放
```

这保证了C只要正确使用 `PersistentStore`，就不会出现服务器只修改内存却忘记持久化的问题。
