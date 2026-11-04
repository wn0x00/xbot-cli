param(
  [string]$ShadowBotDir = $(if ($env:XBOT_SHADOWBOT_DIR) { $env:XBOT_SHADOWBOT_DIR } else { 'E:\影刀\shadowbot-6.3.39' }),
  [string]$DotnetRoot = 'C:\Program Files\dotnet'
)
# Build the in-process payload: native shim (Rust cdylib) + managed injectee
# (compiled in-memory via Roslyn, no dotnet SDK required — .NET 6 core/desktop
# runtime assemblies + ShadowBot reference assemblies are enough).
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot
$payload=Join-Path $root 'payload'
$deploy=Join-Path $payload 'deploy'
New-Item -ItemType Directory -Force $deploy | Out-Null

# 1. shim → payload/deploy/XbotShim.dll
cargo build --release --manifest-path (Join-Path $payload 'shim/Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'payload shim build failed' }
Copy-Item (Join-Path $payload 'shim/target/release/XbotShim.dll') $deploy -Force
Copy-Item (Join-Path $payload 'injectee/XbotInjectee.runtimeconfig.json') $deploy -Force

# 2. injectee → payload/deploy/XbotInjectee.dll (+ .identity)
$references=@(Get-ChildItem (Join-Path $PSHOME 'ref') -Filter '*.dll' | ForEach-Object FullName)
$references+=@((Join-Path $PSHOME 'Microsoft.CodeAnalysis.dll'),(Join-Path $PSHOME 'Microsoft.CodeAnalysis.CSharp.dll'))
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Linq;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
public static class BuildInjectee {
 private static bool Managed(string path) { try{System.Reflection.AssemblyName.GetAssemblyName(path);return true;}catch(BadImageFormatException){return false;} }
 public static string Build(string payload,string core,string desktop,string shadowBot) {
   var parse=CSharpParseOptions.Default.WithLanguageVersion(LanguageVersion.CSharp10);
   var trees=Directory.GetFiles(Path.Combine(payload,"injectee","src"),"*.cs").Select(p=>CSharpSyntaxTree.ParseText(File.ReadAllText(p),parse,p)).ToList();
   trees.Add(CSharpSyntaxTree.ParseText("global using System; global using System.Collections.Generic; global using System.IO; global using System.Linq; global using System.Net.Http; global using System.Threading; global using System.Threading.Tasks;",parse));
   var paths=Directory.GetFiles(core,"*.dll").Concat(Directory.GetFiles(desktop,"*.dll"))
     .Concat(new[]{Path.Combine(shadowBot,"Caliburn.Micro.dll"),Path.Combine(shadowBot,"ShadowBot.Components.Protocol.dll")})
     .Where(Managed).GroupBy(Path.GetFileName,StringComparer.OrdinalIgnoreCase).Select(g=>g.Last());
   var refs=paths.Select(p=>MetadataReference.CreateFromFile(p));
   var source=string.Join("\n",trees.Select(t=>t.ToString()))+File.ReadAllText(Path.Combine(payload,"contracts","client-api.json"));
   var hash=Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(source))).Substring(0,16);
   var identity="XbotInjectee.GuanZhu_"+hash;
   var compilation=CSharpCompilation.Create(identity,trees,refs,new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary,optimizationLevel:OptimizationLevel.Release,platform:Platform.X64));
   var output=Path.Combine(payload,"deploy","XbotInjectee.dll");
   var resource=new ResourceDescription("XbotInjectee.ClientContracts",()=>File.OpenRead(Path.Combine(payload,"contracts","client-api.json")),true);
   using(var stream=File.Create(output+".build")) {
     var result=compilation.Emit(stream,manifestResources:new[]{resource});
     var errors=result.Diagnostics.Where(d=>d.Severity==DiagnosticSeverity.Error).Select(d=>d.ToString()).ToArray();
     if(!result.Success)throw new Exception(string.Join(Environment.NewLine,errors));
   }
   File.Move(output+".build",output,true);
   File.WriteAllText(Path.Combine(payload,"deploy","XbotInjectee.identity"),identity);
   return output;
 }
}
'@ -ReferencedAssemblies $references -CompilerOptions '/nowarn:1701,1702'
$core=Get-ChildItem (Join-Path $DotnetRoot 'shared\Microsoft.NETCore.App') -Directory | Where-Object Name -Like '6.*' | Sort-Object {[version]$_.Name} -Descending | Select-Object -First 1
$desktop=Get-ChildItem (Join-Path $DotnetRoot 'shared\Microsoft.WindowsDesktop.App') -Directory | Where-Object Name -Like '6.*' | Sort-Object {[version]$_.Name} -Descending | Select-Object -First 1
if(-not $core -or -not $desktop){throw '.NET 6 core and desktop runtime assemblies are required'}
if(-not (Test-Path (Join-Path $ShadowBotDir 'Caliburn.Micro.dll'))){throw "ShadowBot reference assemblies not found under $ShadowBotDir (set XBOT_SHADOWBOT_DIR)"}
$out=[BuildInjectee]::Build($payload,$core.FullName,$desktop.FullName,$ShadowBotDir)
Write-Host "payload ready: $out ($(Get-Content (Join-Path $deploy 'XbotInjectee.identity')))"
