// Standalone fault injector. It never loads UIA or accesses application windows.
using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Web.Script.Serialization;

public class Win32FaultProvider {
    public static int Main(string[] ignored) {
        var serializer = new JavaScriptSerializer { MaxJsonLength = 32 * 1024 * 1024 };
        var input = serializer.Deserialize<Dictionary<string, object>>(Console.In.ReadToEnd());
        var request = input["request"] as Dictionary<string, object>;
        if (request == null) {
            Console.WriteLine("{\"ok\":false,\"code\":\"WIN32_PROVIDER_ERROR\"}");
            return 1;
        }
        File.WriteAllText((string)request["receipt_path"], "issued");
        File.AppendAllText(Environment.GetEnvironmentVariable("XBOT_TEST_COUNT"), "issued\n");
        var mode = Environment.GetEnvironmentVariable("XBOT_TEST_MODE");
        if (mode == "timeout") Thread.Sleep(5000);
        if (mode == "error") {
            Console.WriteLine("{\"ok\":false,\"code\":\"WIN32_PROVIDER_ERROR\"}");
            return 1;
        }
        var snapshot = (Dictionary<string, object>)request["snapshot"];
        Console.WriteLine(serializer.Serialize(new {
            ok = true, data = new {
                command = "win32.click", backend = "uiautomation", applied = true,
                outcome = "action_returned", source_snapshot_id = snapshot["snapshot_id"],
                @ref = request["ref"]
            }
        }));
        return 0;
    }
}
