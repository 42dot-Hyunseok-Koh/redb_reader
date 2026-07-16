# redb_reader

Cargo를 로컬에 설치하지 않고 Windows 64-bit용 `redb_reader.exe`를 만들기 위한 프로젝트입니다.

업로드된 `dm_cluster_persistent.db`는 SQLite/RocksDB가 아니라 `redb` 파일로 보입니다. 이 Reader는 다음을 지원합니다.

- DB 내부 문자열 스캔
- redb table 목록 출력
- table을 몇 가지 타입 조합으로 열어 raw key/value 덤프

## Cargo 없이 Windows exe 만들기

1. GitHub에서 새 private repository를 만듭니다.
2. 이 zip 내용을 repository root에 업로드합니다.
3. GitHub Actions 탭에서 `Build Windows exe` workflow를 실행합니다.
4. 완료 후 Artifacts에서 `redb_reader-windows-x64`를 다운로드합니다.
5. 압축 안의 `redb_reader.exe`를 사용합니다.

## 사용법

PowerShell:

```powershell
.\redb_reader.exe C:\path\dm_cluster_persistent.db scan-strings
.\redb_reader.exe C:\path\dm_cluster_persistent.db list
.\redb_reader.exe C:\path\dm_cluster_persistent.db dump
.\redb_reader.exe C:\path\dm_cluster_persistent.db dump log_metadata --limit 200
```

결과를 파일로 저장하려면:

```powershell
.\redb_reader.exe C:\path\dm_cluster_persistent.db dump --limit 1000 > dump.txt
```

## 주의

- 원본 DB는 먼저 복사본으로 작업하세요.
- `redb`는 typed table 방식입니다. DB를 만든 Rust 코드의 정확한 `TableDefinition<K, V>` 타입을 알아야 완전한 역직렬화가 가능합니다.
- 이 Reader는 우선 raw key/value를 확인하기 위한 도구입니다.

현재 시도하는 타입 추정:

- `TableDefinition<&str, Vec<u8>>`
- `TableDefinition<u64, Vec<u8>>`
- `TableDefinition<&str, &str>`
