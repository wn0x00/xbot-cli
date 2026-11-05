using System.Globalization;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Security.Cryptography;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;

internal sealed class AdapterError : Exception
{
    public string Code { get; }
    public AdapterError(string code, string message) : base(message) => Code = code;
}

internal static class Program
{
    private static readonly JsonSerializerOptions Json = new()
    {
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        MaxDepth = 256
    };
    private static readonly JsonDocumentOptions Document = new() { MaxDepth = 256 };
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private static readonly HashSet<string> Keywords = new(
        ("False None True __debug__ and as assert async await break class continue def del elif else except finally " +
         "for from global if import in is lambda nonlocal not or pass raise return try while with yield").Split(' '));
    private const int MaxBytes = 64 * 1024 * 1024;

    private static int Main(string[] args)
    {
        Console.OutputEncoding = new UTF8Encoding(false);
        bool written = false;
        try
        {
            if (args.Length != 1) throw new AdapterError("BAD_ARGS", "Expected a request JSON file.");
            JsonObject request = Object(Parse(Read(args[0])), "request");
            object data = Text(request, "action") switch
            {
                "catalog" => Catalog(Text(request, "installation")),
                "context" => Resolve(request),
                "plan" => Plan(request),
                "apply" => Apply(request, ref written),
                _ => throw new AdapterError("BAD_ARGS", "Unknown adapter action.")
            };
            Console.WriteLine(JsonSerializer.Serialize(new { ok = true, data }, Json));
            return 0;
        }
        catch (Exception error)
        {
            Console.WriteLine(JsonSerializer.Serialize(new
            {
                ok = false,
                error = new { code = error is AdapterError known ? known.Code : "PARAMETER_ADAPTER_FAILED", message = error.Message },
                data = new { written }
            }, Json));
            return 1;
        }
    }

    private static byte[] Read(string file)
    {
        var info = new FileInfo(file);
        if (!info.Exists || info.Length > MaxBytes)
            throw new AdapterError("FILE_READ", "Missing file or file exceeds 64 MiB: " + file);
        return File.ReadAllBytes(file);
    }

    private static JsonNode Parse(byte[] bytes)
    {
        int bom = bytes.AsSpan().StartsWith(new byte[] { 0xef, 0xbb, 0xbf }) ? 3 : 0;
        return JsonNode.Parse(Utf8.GetString(bytes.AsSpan(bom)), documentOptions: Document)
            ?? throw new AdapterError("BAD_JSON", "Expected JSON, not null.");
    }

    private static JsonObject Object(JsonNode? node, string label) =>
        node as JsonObject ?? throw new AdapterError("BAD_INPUT", label + " must be an object.");

    private static string Text(JsonObject node, string key) =>
        node[key] is JsonValue value && value.TryGetValue<string>(out string? text) && text != null
            ? text : throw new AdapterError("BAD_INPUT", key + " must be a string.");

    private static string Nonempty(JsonObject node, string key)
    {
        string value = Text(node, key);
        if (string.IsNullOrWhiteSpace(value)) throw new AdapterError("BAD_INPUT", key + " cannot be empty.");
        return value;
    }

    private static JsonNode? Clone(JsonNode? node) => node == null ? null : JsonNode.Parse(node.ToJsonString(Json), documentOptions: Document);
    private static string Sha(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
    private static bool Equal(JsonNode? left, JsonNode? right) => left?.ToJsonString(Json) == right?.ToJsonString(Json);
    private static void Fail(bool condition, string code, string message)
    {
        if (condition) throw new AdapterError(code, message);
    }

    private static JsonObject Catalog(string installation)
    {
        string file = Path.Combine(installation, "ShadowBot.Runtime.Development.dll");
        byte[] bytes = Read(file);
        using var stream = new MemoryStream(bytes, writable: false);
        using var pe = new PEReader(stream);
        var metadata = pe.GetMetadataReader();
        var directory = pe.PEHeaders.CorHeader?.ResourcesDirectory
            ?? throw new AdapterError("PARAMETER_CATALOG_FAILED", "Missing CLI resource directory.");
        var resources = pe.GetSectionData(directory.RelativeVirtualAddress).GetContent(0, directory.Size);
        var types = new SortedDictionary<string, JsonObject>(StringComparer.Ordinal);
        int builtinCount = 0;
        const string prefix = "ShadowBot.Runtime.Development.Resources.Zh_CN.";
        foreach (var handle in metadata.ManifestResources)
        {
            var resource = metadata.GetManifestResource(handle);
            string name = metadata.GetString(resource.Name);
            if (!name.StartsWith(prefix, StringComparison.Ordinal)) continue;
            bool builtin = name == prefix + "buildin_types.json";
            if (!builtin && !name.EndsWith(".blocks.json", StringComparison.Ordinal)) continue;
            Fail(!resource.Implementation.IsNil, "PARAMETER_CATALOG_FAILED", "External resource: " + name);
            long offset = resource.Offset;
            Fail(offset < 0 || offset > resources.Length - 4, "PARAMETER_CATALOG_FAILED", "Invalid resource offset.");
            int size = BitConverter.ToInt32(resources.AsSpan().Slice((int)offset, 4));
            Fail(size < 0 || size > resources.Length - offset - 4, "PARAMETER_CATALOG_FAILED", "Invalid resource length.");
            JsonNode payload = Parse(resources.AsSpan().Slice((int)offset + 4, size).ToArray());
            JsonArray? definitions = builtin ? payload as JsonArray : payload["types"] as JsonArray;
            if (definitions == null) continue;
            foreach (JsonNode? item in definitions)
            {
                var definition = Object(item, "type definition");
                string id = Nonempty(definition, "name");
                if (id == "password") continue;
                var entry = new JsonObject
                {
                    ["id"] = id,
                    ["name"] = definition["localName"]?.GetValue<string>() ?? id,
                    ["scope"] = Clone(definition["scope"]),
                    ["base"] = Clone(definition["base"]),
                    ["source"] = name
                };
                Fail(types.TryGetValue(id, out var old) && !Equal(old, entry),
                    "PARAMETER_CATALOG_FAILED", "Conflicting type definitions: " + id);
                types[id] = entry;
                if (builtin) builtinCount++;
            }
        }
        Fail(builtinCount == 0, "PARAMETER_CATALOG_FAILED", "No built-in type catalog found.");
        return new JsonObject
        {
            ["types"] = new JsonArray(types.Values.Select(item => (JsonNode)item).ToArray()),
            ["count"] = types.Count,
            ["artifact"] = file,
            ["sha256"] = Sha(bytes),
            ["note"] = "Static client type definitions, not a universal parameter-selectability list. Missing scope is unspecified."
        };
    }

    private static void NoLinks(string path)
    {
        for (string? current = Path.GetFullPath(path); current != null; current = Path.GetDirectoryName(current))
        {
            if (File.Exists(current) || Directory.Exists(current))
                Fail((File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0,
                    "PARAMETER_PATH_UNSAFE", "Refusing a reparse-point path: " + current);
        }
    }

    private static JsonObject Resolve(JsonObject request)
    {
        string appId = Nonempty(request, "app_id");
        Fail(!Guid.TryParseExact(appId, "D", out _), "BAD_APP_ID", "Invalid app UUID.");
        string users = Path.GetFullPath(Nonempty(request, "users_root"));
        NoLinks(users);
        var matches = new List<string>();
        foreach (string user in Directory.EnumerateDirectories(users))
        {
            string package = Path.Combine(user, "apps", appId, "xbot_robot");
            if (!File.Exists(Path.Combine(package, "package.json"))) continue;
            NoLinks(package);
            var metadata = Object(Parse(Read(Path.Combine(package, "package.json"))), "package");
            Fail(Text(metadata, "uuid") != appId, "APP_MISMATCH", "App directory and package UUID differ.");
            matches.Add(package);
        }
        Fail(matches.Count != 1, "PARAMETER_APP_AMBIGUOUS",
            "Expected exactly one saved app directory; found " + matches.Count + ".");
        string appDirectory = Path.GetDirectoryName(matches[0])!;
        Fail(Directory.Exists(appDirectory + "_temp"), "VIEW_MODE_READONLY", "A read-only View copy exists.");
        request["package_dir"] = matches[0];
        return Context(request);
    }

    private static JsonObject Context(JsonObject request)
    {
        string package = Path.GetFullPath(Nonempty(request, "package_dir"));
        string appId = Nonempty(request, "app_id");
        string flowId = Nonempty(request, "flow_id");
        Fail(flowId != Path.GetFileName(flowId) || flowId.Contains(':') || flowId is "." or "..",
            "BAD_INPUT", "Unsafe flow ID.");
        string manifest = Path.Combine(package, "package.json");
        string source = Path.Combine(package, ".dev", flowId + ".flow.json");
        NoLinks(manifest);
        NoLinks(source);
        var metadata = Object(Parse(Read(manifest)), "package");
        Fail(Text(metadata, "uuid") != appId, "APP_MISMATCH", "Package UUID mismatch.");
        var flows = metadata["flows"] as JsonArray ?? throw new AdapterError("BAD_JSON", "package.flows must be an array.");
        var selected = flows.OfType<JsonObject>().Where(flow => flow["filename"]?.GetValue<string>() == flowId).ToArray();
        Fail(selected.Length != 1, "FLOW_NOT_FOUND", "Expected exactly one matching saved flow.");
        string kind = Text(selected[0], "kind").ToLowerInvariant();
        Fail(kind != "visual" && kind != "code", "FLOW_KIND_MISMATCH", "Unsupported saved flow kind: " + kind);
        return new JsonObject
        {
            ["package_dir"] = package, ["manifest"] = manifest, ["source"] = source,
            ["app_id"] = appId, ["flow_id"] = flowId, ["flow_kind"] = kind,
            ["is_app_main"] = metadata["robot_type"]?.GetValue<string>() == "app" &&
                metadata["startup"]?.GetValue<string>() == flowId
        };
    }

    private static void Identifier(string name)
    {
        Fail(name.Length == 0 || name != name.Normalize(NormalizationForm.FormKC) || Keywords.Contains(name),
            "BAD_INPUT", "Invalid Python parameter identifier: " + name);
        bool first = true;
        foreach (Rune rune in name.EnumerateRunes())
        {
            var category = Rune.GetUnicodeCategory(rune);
            bool start = category is UnicodeCategory.UppercaseLetter or UnicodeCategory.LowercaseLetter
                or UnicodeCategory.TitlecaseLetter or UnicodeCategory.ModifierLetter or UnicodeCategory.OtherLetter
                or UnicodeCategory.LetterNumber || rune.Value is 0x1885 or 0x1886 or 0x2118 or 0x212e or 0x309b or 0x309c;
            bool continuation = start || category is UnicodeCategory.NonSpacingMark or UnicodeCategory.SpacingCombiningMark
                or UnicodeCategory.DecimalDigitNumber or UnicodeCategory.ConnectorPunctuation ||
                rune.Value is 0x00b7 or 0x0387 or 0x1369 or 0x136a or 0x136b or 0x136c or 0x136d
                    or 0x136e or 0x136f or 0x1370 or 0x1371 or 0x19da;
            Fail(rune.Value != '_' && !(first ? start : continuation), "BAD_INPUT", "Invalid Python parameter identifier: " + name);
            first = false;
        }
    }

    private static (string kind, string value) Default(string type, JsonObject? definition, string direction)
    {
        if (direction == "Out") return ("Expression", "");
        return type switch
        {
            "str" or "file" or "directory" => ("Text", ""),
            "int" => ("Expression", "0"),
            "float" => ("Expression", "0.0"),
            "bool" => ("Expression", "False"),
            "list" => ("Expression", "[]"),
            "dict" => ("Expression", "{}"),
            "bytes" => ("Expression", "b''"),
            _ => ("Expression", definition?["base"]?.GetValue<string>() == "list" ? "[]" : "None")
        };
    }

    private static JsonObject Canonical(JsonObject parameter) => new()
    {
        ["name"] = Text(parameter, "name"), ["direction"] = Text(parameter, "direction"),
        ["type"] = Text(parameter, "type"), ["kind"] = parameter["kind"]?.GetValue<string>() ?? "Expression",
        ["value"] = parameter["value"]?.GetValue<string>() ?? "",
        ["description"] = parameter["description"]?.GetValue<string>() ?? ""
    };

    private static (int start, int end) ParameterRange(byte[] bytes)
    {
        int bom = bytes.AsSpan().StartsWith(new byte[] { 0xef, 0xbb, 0xbf }) ? 3 : 0;
        var reader = new Utf8JsonReader(bytes.AsSpan(bom), new JsonReaderOptions { MaxDepth = 256 });
        (int start, int end)? range = null;
        while (reader.Read())
        {
            if (reader.TokenType != JsonTokenType.PropertyName || reader.CurrentDepth != 1 || !reader.ValueTextEquals("parameters")) continue;
            Fail(range != null, "BAD_JSON", "Duplicate top-level parameters field.");
            reader.Read();
            Fail(reader.TokenType != JsonTokenType.StartArray, "BAD_JSON", "parameters must be an array.");
            int start = checked((int)reader.TokenStartIndex) + bom;
            reader.Skip();
            range = (start, checked((int)reader.BytesConsumed) + bom);
        }
        return range ?? throw new AdapterError("BAD_JSON", "Missing parameters array.");
    }

    private static JsonObject Plan(JsonObject request)
    {
        JsonObject context = Context(request);
        string source = Text(context, "source");
        byte[] before = Read(source);
        var model = Object(Parse(before), "flow");
        Fail(Text(model, "kind") != Text(context, "flow_kind"), "FLOW_KIND_MISMATCH", "Flow kind differs from package.");
        if (Text(model, "kind") == "visual")
            Fail(model["blocks"] is not JsonArray, "BAD_JSON", "VisualFlow blocks must be an array.");
        var original = model["parameters"] as JsonArray ?? throw new AdapterError("BAD_JSON", "parameters must be an array.");
        var next = (JsonArray)Clone(original)!;
        var types = (request["types"] as JsonArray ?? throw new AdapterError("BAD_INPUT", "Missing types."))
            .OfType<JsonObject>().ToDictionary(type => Text(type, "id"), StringComparer.Ordinal);
        var operations = request["operations"] as JsonArray ?? throw new AdapterError("BAD_INPUT", "Missing operations.");
        Fail(operations.Count == 0, "BAD_INPUT", "operations cannot be empty.");
        var identities = new HashSet<string>(StringComparer.Ordinal);
        foreach (JsonNode? item in next)
        {
            string identity = Nonempty(Object(item, "parameter"), "name").Normalize(NormalizationForm.FormKC);
            Fail(!identities.Add(identity), "PARAMETER_CONFLICT", "Duplicate normalized parameter name.");
        }
        var changes = new JsonArray();
        var warnings = new HashSet<string>();
        foreach (JsonNode? item in operations)
        {
            JsonObject operation = Object(item, "operation");
            string op = Text(operation, "op");
            string name = Nonempty(operation, "name");
            foreach (var field in operation)
                Fail(!new[] { "op", "name", "direction", "type", "kind", "value", "description" }.Contains(field.Key),
                    "BAD_INPUT", "Unknown operation field: " + field.Key);
            int index = next.Select((node, i) => (node, i))
                .Where(pair => pair.node?["name"]?.GetValue<string>() == name).Select(pair => pair.i).DefaultIfEmpty(-1).First();
            if (op == "remove")
            {
                Fail(operation.Count != 2, "BAD_INPUT", "remove only accepts op and name.");
                if (index >= 0) next.RemoveAt(index);
                changes.Add(new JsonObject { ["op"] = "remove", ["name"] = name, ["changed"] = index >= 0 });
                continue;
            }
            Fail(op != "upsert", "BAD_INPUT", "op must be upsert or remove.");
            Identifier(name);
            Fail(next.OfType<JsonObject>().Any(p => Text(p, "name") != name &&
                Text(p, "name").Normalize(NormalizationForm.FormKC) == name), "PARAMETER_CONFLICT", "Normalized name collision.");
            string type = Nonempty(operation, "type");
            Fail(type == "password", "BAD_INPUT", "password is not a supported flow parameter type.");
            string direction = Text(operation, "direction");
            Fail(direction != "In" && direction != "Out", "BAD_INPUT", "direction must be In or Out.");
            JsonObject? old = index >= 0 ? Object(next[index], "parameter") : null;
            bool same = old?["type"]?.GetValue<string>() == type && old?["direction"]?.GetValue<string>() == direction;
            types.TryGetValue(type, out var definition);
            Fail(definition == null && !same, "BAD_INPUT", "Unknown exact client type ID: " + type);
            string? scope = definition?["scope"]?.GetValue<string>();
            if (scope != null)
            {
                Fail(scope != "appMainFlowParameter" && !same, "BAD_INPUT", "Unverified client type scope: " + scope);
                if (scope == "appMainFlowParameter" && !context["is_app_main"]!.GetValue<bool>())
                    warnings.Add(type + " is declared for app main-flow parameters; subflow metadata was accepted in client 6.3.39, but editor selection rules may differ.");
                else if (scope != "appMainFlowParameter")
                    warnings.Add("Preserved existing parameter with an unverified scope: " + name);
            }
            var fallback = Default(type, definition, direction);
            JsonObject parameter = old == null ? new JsonObject() : (JsonObject)Clone(old)!;
            parameter["name"] = name;
            parameter["direction"] = direction;
            parameter["type"] = type;
            string kind = operation.ContainsKey("kind") ? Text(operation, "kind")
                : same ? old?["kind"]?.GetValue<string>() ?? fallback.kind : fallback.kind;
            if (operation.ContainsKey("value") && !operation.ContainsKey("kind"))
            {
                Fail(type == "any" && Text(operation, "value").Length > 0, "BAD_INPUT", "any with a value requires explicit kind.");
                if (!same) kind = fallback.kind;
            }
            string value = operation.ContainsKey("value") ? Text(operation, "value")
                : same ? old?["value"]?.GetValue<string>() ?? "" : fallback.value;
            Fail(kind != "Text" && kind != "Expression", "BAD_INPUT", "kind must be Text or Expression.");
            Fail(direction == "In" && kind == "Expression" && string.IsNullOrWhiteSpace(value),
                "BAD_INPUT", "In Expression initializer cannot be empty.");
            if (!same || operation.ContainsKey("kind")) parameter["kind"] = kind;
            if (!same || operation.ContainsKey("value")) parameter["value"] = value;
            if (old == null || operation.ContainsKey("description"))
                parameter["description"] = operation.ContainsKey("description") ? Text(operation, "description") : "";
            bool changed = !Equal(old, parameter);
            if (index >= 0) next[index] = parameter;
            else next.Add(parameter);
            changes.Add(new JsonObject { ["op"] = old == null ? "add" : "update", ["name"] = name, ["type"] = type, ["changed"] = changed });
        }
        bool netChange = !Equal(original, next);
        var range = ParameterRange(before);
        string newline = Utf8.GetString(before).Contains("\r\n") ? "\r\n" : "\n";
        var pretty = new JsonSerializerOptions(Json) { WriteIndented = true };
        string replacement = next.ToJsonString(pretty).Replace("\n", newline + "  ");
        byte[] content = before;
        if (netChange)
        {
            using var output = new MemoryStream();
            output.Write(before.AsSpan(0, range.start));
            output.Write(Utf8.GetBytes(replacement));
            output.Write(before.AsSpan(range.end));
            content = output.ToArray();
        }
        context["changed"] = netChange;
        context["before_sha256"] = Sha(before);
        context["after_sha256"] = Sha(content);
        context["manifest_sha256"] = Sha(Read(Text(context, "manifest")));
        context["content_base64"] = Convert.ToBase64String(content);
        context["before_parameters"] = new JsonArray(original.OfType<JsonObject>().Select(p => (JsonNode)Canonical(p)).ToArray());
        context["parameters"] = new JsonArray(next.OfType<JsonObject>().Select(p => (JsonNode)Canonical(p)).ToArray());
        context["changes"] = changes;
        context["warnings"] = new JsonArray(warnings.Select(w => (JsonNode?)JsonValue.Create(w)).ToArray());
        return context;
    }

    private static object Apply(JsonObject request, ref bool written)
    {
        var plan = Object(request["plan"], "plan");
        JsonObject context = Context(plan);
        string source = Text(context, "source");
        string manifest = Text(context, "manifest");
        byte[] before = Read(source);
        void Check()
        {
            Fail(Sha(Read(source)) != Text(plan, "before_sha256") || Sha(Read(manifest)) != Text(plan, "manifest_sha256"),
                "PARAMETER_CONFLICT", "App metadata changed after planning; nothing was replaced.");
        }
        Check();
        byte[] after = Convert.FromBase64String(Text(plan, "content_base64"));
        Fail(Sha(after) != Text(plan, "after_sha256"), "BAD_INPUT", "Plan content hash mismatch.");
        var oldRange = ParameterRange(before);
        var newRange = ParameterRange(after);
        Fail(!before.AsSpan(0, oldRange.start).SequenceEqual(after.AsSpan(0, newRange.start)) ||
            !before.AsSpan(oldRange.end).SequenceEqual(after.AsSpan(newRange.end)),
            "BAD_INPUT", "Plan attempted to change non-parameter bytes.");
        string temporary = source + ".xbot-cli-" + Guid.NewGuid().ToString("N");
        try
        {
            using (var stream = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                stream.Write(after);
                stream.Flush(flushToDisk: true);
            }
            Check();
            File.Replace(temporary, source, null);
            written = true;
            Fail(Sha(Read(source)) != Text(plan, "after_sha256"), "PARAMETER_CONFLICT", "File changed immediately after replacement.");
            return new { written = true, path = source, sha256 = Text(plan, "after_sha256") };
        }
        finally
        {
            if (File.Exists(temporary)) File.Delete(temporary);
        }
    }
}
