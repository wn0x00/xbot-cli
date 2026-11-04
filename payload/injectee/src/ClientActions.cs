using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text.Json;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Threading;

namespace XbotInjectee;
internal static class ClientActions {
    private static readonly BindingFlags PublicInstance=BindingFlags.Public|BindingFlags.Instance;
    private static readonly Dictionary<string,string[]> Settings=new() {
        ["user"]=new[]{"IntervalTime","IsShowCloseTip","ShowVirtualDesktopTip"},
        ["language"]=new[]{"Culture"},
        ["theme"]=new[]{"ThemeMode"},
        ["hotkeys"]=new[]{"Hotkeys"},
        ["anchor"]=new[]{"SupportAutoAnchor"},
        ["selector"]=new[]{"EnableAiSelector"},
        ["recording"]=new[]{"IsOpenRecording","SaveMode","IgnoreRecordManualOrHotkey","StoragePath","VideoMaxSize","SaveDuration","IsAutoClearFile"},
        ["error-email"]=new[]{"IsEnable","ReceiverAddress","UseCustomSender","EmailProvider","Ip","Port","UseSsl","UserName","AuthCode","SenderAddress"},
        ["error-dingtalk"]=new[]{"IsEnable","Webhook","DingTalkRobotSecurityPolicy","Keyword","Secret"},
        ["error-feishu"]=new[]{"IsEnable","Webhook","Secret"},
        ["error-wecom"]=new[]{"IsEnable","Webhook"},
        ["error-app"]=new[]{"IsEnable","AppId","AppName"},
    };
    private static readonly Dictionary<string,string> ErrorTypes=new() {
        ["error-email"]="SendEmailRule",["error-dingtalk"]="SendDingTalkRule",["error-feishu"]="SendFeiShuRule",["error-wecom"]="SendWXWorkRule",["error-app"]="ExecuteAppRule"
    };
    private static JsonElement[] Contracts() {
        using var stream=typeof(ClientActions).Assembly.GetManifestResourceStream("XbotInjectee.ClientContracts")??throw new InvalidOperationException("API contract resource missing");
        using var doc=JsonDocument.Parse(stream);
        return doc.RootElement.EnumerateArray().Select(x=>x.Clone()).ToArray();
    }
    private static Type TypeOf(string name) {
        foreach(var assembly in AppDomain.CurrentDomain.GetAssemblies()){var type=assembly.GetType(name,false);if(type!=null)return type;}
        throw new NotSupportedException("client type not loaded: "+name);
    }
    private static object Get(object target,string name)=>target?.GetType().GetProperty(name,PublicInstance)?.GetValue(target);
    private static object CurrentUser()=>TypeOf("ShadowBot.Runtime.User").GetProperty("Current",BindingFlags.Public|BindingFlags.Static)?.GetValue(null)
        ??throw new InvalidOperationException("current client user is unavailable");
    private static object Service(string name) {
        var ioc=TypeOf("Caliburn.Micro.IoC");
        var factory=ioc.GetField("GetInstance",BindingFlags.Public|BindingFlags.Static)?.GetValue(null) as Func<Type,string,object>;
        return factory?.Invoke(TypeOf(name),null)??throw new NotSupportedException("client service unavailable: "+name);
    }
    private static T OnUi<T>(Func<T> action) {
        var app=Application.Current??throw new InvalidOperationException("no live client application");
        if(app.Dispatcher.CheckAccess())return action();
        var operation=app.Dispatcher.InvokeAsync(action);
        if(!operation.Task.Wait(TimeSpan.FromSeconds(10))) {
            if(operation.Abort())throw new TimeoutException("client UI busy; queued operation cancelled before execution");
            throw new TimeoutException("client UI operation already started; outcome unknown, inspect before retrying");
        }
        return operation.Task.GetAwaiter().GetResult();
    }
    private static object Await(object result) {
        if(result is not Task task)return result;
        task.WaitAsync(TimeSpan.FromSeconds(90)).GetAwaiter().GetResult();
        return task.GetType().GetProperty("Result")?.GetValue(task);
    }
    private static JsonElement Snapshot(object value) {
        var converter=TypeOf("Newtonsoft.Json.JsonConvert");
        var serialize=converter.GetMethod("SerializeObject",new[]{typeof(object)});
        var text=(string)serialize.Invoke(null,new[]{value});
        using var document=JsonDocument.Parse(text);return document.RootElement.Clone();
    }
    public static object Invoke(JsonElement body) {
        var operation=body.GetProperty("operation").GetString();
        var contract=Contracts().FirstOrDefault(c=>c.GetProperty("operation").GetString()==operation);
        if(contract.ValueKind==JsonValueKind.Undefined)throw new ArgumentException("operation is not allowlisted");
        var parts=operation.Split('.');
        var apiType=TypeOf("ShadowBot.Components.Swagger.Api."+parts[0]);
        var methods=apiType.GetMethods(PublicInstance).Where(m=>m.Name==parts[1]&&!m.IsGenericMethod).ToArray();
        if(methods.Length!=1)throw new NotSupportedException("client method signature changed");
        var arguments=ClientContracts.BindArguments(methods[0],body.GetProperty("arguments"));
        var api=OnUi(()=>{
            var apiClient=Get(CurrentUser(),"ApiClient");
            if(apiClient==null)apiClient=Service("ShadowBot.Components.Protocol.Swagger.IApiClient");
            return Activator.CreateInstance(apiType,new[]{apiClient});
        });
        // API wrappers include synchronous HTTP methods: never invoke them on WPF's dispatcher.
        var pending=methods[0].Invoke(api,arguments);
        var value=Snapshot(Await(pending));
        if(value.ValueKind==JsonValueKind.Object && value.TryGetProperty("Code",out var code) && code.ValueKind==JsonValueKind.Number && code.GetInt32()!=0)
            return new {ok=false,operation,detail="client API rejected request",data=value};
        return new {ok=true,operation,data=value};
    }
    private record SettingTarget(object Value,object SaveOwner,string SaveMethod,Type GenericType=null,bool NewRule=false);
    private static SettingTarget Target(string scope) {
        if(scope=="user"||scope=="hotkeys") {var settings=Get(CurrentUser(),"Settings")??throw new NotSupportedException("User.Settings unavailable");return new(settings,settings,"Save");}
        if(scope=="language") {
            var config=TypeOf("ShadowBot.Runtime.Product").GetProperty("Client",BindingFlags.Public|BindingFlags.Static)?.GetValue(null)
                ??throw new NotSupportedException("live language configuration unavailable");
            return new(config,config,"Save");
        }
        if(scope.StartsWith("error-")) {
            var center=Service("ShadowBot.Shell.Console.Common.AppErrorHandling.IAppErrorHandlingCenter");
            var rules=Get(center,"GeneralErrorHandlingRules") as IEnumerable??throw new NotSupportedException("error handling rules unavailable");
            var rule=rules.Cast<object>().FirstOrDefault(r=>r.GetType().Name==ErrorTypes[scope]);
            var fresh=rule==null;
            rule??=Activator.CreateInstance(TypeOf("ShadowBot.Shell.Console.Common.AppErrorHandling."+ErrorTypes[scope]));
            return new(rule,center,"SaveGeneralErrorHandlingRules",null,fresh);
        }
        var configType=scope switch {"anchor"=>"ShadowBot.Runtime.Configuration.Client.AnchorClientConfig","selector"=>"ShadowBot.Runtime.Configuration.Client.SelectorLocalConfig","recording"=>"ShadowBot.Runtime.Capture.CaptureClientConfig",_=>throw new ArgumentException("unknown settings scope")};
        var managerType=TypeOf("ShadowBot.Runtime.Configuration.Client.LocalConfigManager");
        var manager=managerType.GetProperty("Instance",BindingFlags.Public|BindingFlags.Static)?.GetValue(null)??throw new NotSupportedException("LocalConfigManager.Instance missing");
        var get=managerType.GetMethods(PublicInstance).Single(m=>m.Name=="GetOrCreate"&&m.IsGenericMethod&&m.GetParameters().Length==0).MakeGenericMethod(TypeOf(configType));
        return new(get.Invoke(manager,null)??throw new NotSupportedException("local settings not loaded"),manager,"Save",TypeOf(configType));
    }
    private static bool Secret(string property)=>new[]{"password","token","secret","authcode","webhook"}.Any(s=>property.Contains(s,StringComparison.OrdinalIgnoreCase));
    private static object DescribeScope(string scope,bool values) {
        if(scope=="theme") {
            if(values)return new {ThemeMode=TypeOf("ShadowBot.Runtime.ThemeConfig").GetMethod("Get",BindingFlags.Public|BindingFlags.Static).Invoke(null,null).ToString()};
            return new {ThemeMode=new{type="ShadowBot.Shell.ThemeManagement.ThemeMode",writable=false,enumValues=new[]{"Light","Dark","Default"},setCommand="settings theme set"}};
        }
        var target=Target(scope).Value;
        var result=new Dictionary<string,object>();
        foreach(var name in Settings[scope]) {
            var property=target.GetType().GetProperty(name,PublicInstance);
            if(property==null)continue;
            if(values)result[name]=Secret(name)?"<redacted>":property.GetValue(target);
            else result[name]=new {type=property.PropertyType.FullName,writable=(scope=="hotkeys"||property.SetMethod?.IsPublic==true)&&scope!="recording"&&scope!="anchor"&&scope!="selector",secret=Secret(name),enumValues=property.PropertyType.IsEnum?Enum.GetNames(property.PropertyType):null};
        }
        return result;
    }
    public static object SettingsOperation(JsonElement body) {
        var action=body.GetProperty("action").GetString();
        if(action=="schema")return OnUi(()=>{
            var scopes=new Dictionary<string,object>();
            foreach(var scope in Settings.Keys){try{scopes[scope]=new{available=true,properties=DescribeScope(scope,false)};}catch(Exception ex){scopes[scope]=new{available=false,reason=ex.GetBaseException().Message};}}
            return (object)new{ok=true,scopes};
        });
        var name=body.GetProperty("scope").GetString();
        if(!Settings.ContainsKey(name))throw new ArgumentException("unknown settings scope");
        if(action=="get")return OnUi(()=>(object)new{ok=true,scope=name,data=Snapshot(DescribeScope(name,true))});
        if(action!="set")throw new ArgumentException("unknown settings action");
        if(name=="recording"||name=="anchor"||name=="selector"||name=="theme")throw new ArgumentException("use validated settings set recording.*, designer.auto-anchor, designer.ai-selector-enabled or settings theme set for this scope");
        if(name=="hotkeys")return SetHotkeys(body.GetProperty("patch"));
        var prepared=OnUi(()=>{
            var target=Target(name);
            var patch=ClientContracts.PreparePatch(target.Value,Settings[name],body.GetProperty("patch"));
            foreach(var item in patch)if(item.Property.Name=="IntervalTime"&&Convert.ToInt32(item.Value)<=0)throw new ArgumentException("IntervalTime must be positive");
            return (target,patch);
        });
        var target=prepared.target;
        IList rules=target.NewRule?OnUi(()=>Get(target.SaveOwner,"GeneralErrorHandlingRules") as IList):null;
        ClientContracts.CommitPatch(prepared.patch,()=>{
                if(target.NewRule)rules.Add(target.Value);
                var method=target.SaveOwner.GetType().GetMethods(PublicInstance).FirstOrDefault(m=>m.Name==target.SaveMethod&&m.IsGenericMethod==(target.GenericType!=null)&&m.GetParameters().Length==(target.GenericType==null?0:1))
                    ??throw new NotSupportedException("settings save method unavailable");
                if(target.GenericType!=null)method=method.MakeGenericMethod(target.GenericType);
                return method.Invoke(target.SaveOwner,target.GenericType==null?null:new[]{target.Value});
        },work=>OnUi(()=>{work();return true;}),TimeSpan.FromSeconds(90),()=>{if(target.NewRule)rules.Remove(target.Value);});
        return OnUi(()=>(object)new{ok=true,scope=name,data=Snapshot(DescribeScope(name,true)),verification="saved_and_compared_with_live_model",requiresRestart=name=="language"});
    }
    private static object SetHotkeys(JsonElement patch) {
        if(patch.ValueKind!=JsonValueKind.Object||patch.EnumerateObject().Any(p=>p.Name!="Hotkeys")||!patch.TryGetProperty("Hotkeys",out var entries)||entries.ValueKind!=JsonValueKind.Array)
            throw new ArgumentException("expected {Hotkeys:[{Name,ModifierKeys,VirtualKey}]} with existing hotkey names");
        return OnUi(()=>{
            var current=Get(Target("hotkeys").Value,"Hotkeys") as IList??throw new NotSupportedException("current hotkey list unavailable");
            var merged=(IList)ClientContracts.ConvertValue(Snapshot(current),current.GetType());
            var names=new HashSet<string>();
            foreach(var input in entries.EnumerateArray()) {
                if(input.ValueKind!=JsonValueKind.Object||input.EnumerateObject().Any(p=>p.Name!="Name"&&p.Name!="ModifierKeys"&&p.Name!="VirtualKey"))throw new ArgumentException("unknown hotkey field");
                var key=input.GetProperty("Name").GetString();
                if(!names.Add(key))throw new ArgumentException("duplicate hotkey name");
                var existing=merged.Cast<object>().SingleOrDefault(x=>(string)Get(x,"Name")==key)??throw new ArgumentException("unknown hotkey: "+key);
                var fields=input.EnumerateObject().Where(p=>p.Name!="Name");
                foreach(var field in fields) {
                    var value=field.Value.GetInt32();
                    if(field.Name=="VirtualKey"&&(value<1||value>255)||field.Name=="ModifierKeys"&&(value<0||value>15))throw new ArgumentException("invalid hotkey key/modifiers");
                    var property=existing.GetType().GetProperty(field.Name);
                    property.SetValue(existing,ClientContracts.ConvertValue(field.Value,property.PropertyType));
                }
            }
            var pairs=new HashSet<string>();foreach(var item in merged)if(!pairs.Add(Get(item,"ModifierKeys")+":"+Get(item,"VirtualKey")))throw new ArgumentException("hotkey conflicts with another configured shortcut");
            var configType=TypeOf("ShadowBot.Runtime.HotkeyClientConfig");
            configType.GetMethod("Save",PublicInstance).Invoke(Activator.CreateInstance(configType),new[]{merged});
            var read=(IList)configType.GetMethod("Get",BindingFlags.Public|BindingFlags.Static).Invoke(null,null);
            if(Snapshot(read).GetRawText()!=Snapshot(merged).GetRawText())throw new InvalidOperationException("hotkey persistence readback mismatch");
            var readItems=read.Cast<object>().ToArray();current.Clear();foreach(var item in readItems)current.Add(item);
            return (object)new{ok=true,scope="hotkeys",data=Snapshot(new{Hotkeys=read}),verification="persisted_and_read_back",requiresRestart=true};
        });
    }
}
