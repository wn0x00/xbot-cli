# Browser DLL Query Adapter

This small .NET 6 Windows x64 helper directly calls public members in installed
ShadowBot assemblies. It never installs/removes extensions, invokes private
members, patches binaries, or injects into the running client. It is not an
official or version-stable SDK.

The compiled DLL and runtime config are embedded in the Rust executable.
End users need only ShadowBot's bundled .NET 6 Desktop runtime, not a .NET SDK
or PowerShell. Official ShadowBot DLLs are resolved from the selected installed
version directory and are not redistributed.

Rebuild the adapter after editing its sources, using a .NET SDK and a compatible
installed ShadowBot version:

```powershell
node scripts/build-browser-query.cjs 'D:\Program Files\ShadowBot\shadowbot-6.3.39'
```

This updates `src/browser_assets/` and its source/artifact hash manifest.
Ordinary Rust/npm builds and CI consume these verified assets without requiring
ShadowBot or a .NET SDK on the build machine.

Public calls: `XbotEnv.Initialize(false)`,
`PluginManager.IsInstalledExtension`, `BuildInBrowserInfoHelper` and
`ChromiumConfigHelper.CustomBrowserInfos`.

`installed` is the DLL's verdict, not a per-profile extension check. Custom
browser entries have `installed: null` and `registered: true`; this is not proof
that the extension is enabled or connected. `webSupported` means the helper
obtained a compatible pipe identifier, not that the browser is running.
Firefox has no supported pipe mapping in this adapter.
