# Windows exe build steps, no local Cargo required

1. Create a new private GitHub repository.
2. Upload all files from this folder to the repository root.
3. Open the repository's Actions tab.
4. Select `Build Windows exe`.
5. Click `Run workflow`.
6. Download the artifact named `redb_reader-windows-x64`.
7. Extract and run `redb_reader.exe`.

Example:

```powershell
.\redb_reader.exe C:\Users\you\Desktop\dm_cluster_persistent.db list
.\redb_reader.exe C:\Users\you\Desktop\dm_cluster_persistent.db dump --limit 500 > dump.txt
```
