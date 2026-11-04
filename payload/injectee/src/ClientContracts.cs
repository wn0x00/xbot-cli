using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading;
using System.Threading.Tasks;

namespace XbotInjectee;
internal static class ClientContracts {
    private static readonly JsonSerializerOptions Options=new() {PropertyNameCaseInsensitive=false,Converters={new JsonStringEnumConverter()}};
    public static object ConvertValue(JsonElement value,Type type) {
        try {
            if(type==typeof(CancellationToken))return CancellationToken.None;
            object converted;
            if(type==typeof(object)||(!type.IsPrimitive&&!type.IsEnum&&type!=typeof(string)&&type!=typeof(decimal)&&type!=typeof(DateTime)&&type!=typeof(DateTimeOffset)&&type!=typeof(TimeSpan)&&Nullable.GetUnderlyingType(type)==null)) {
                // Client DTOs use Newtonsoft attributes; JsonElement would serialize as {ValueKind:1} in the original wrappers.
                var converter=Type.GetType("Newtonsoft.Json.JsonConvert, Newtonsoft.Json",true);
                var settingsType=Type.GetType("Newtonsoft.Json.JsonSerializerSettings, Newtonsoft.Json",true);
                var settings=Activator.CreateInstance(settingsType);
                var missing=settingsType.GetProperty("MissingMemberHandling");
                missing.SetValue(settings,Enum.Parse(missing.PropertyType,"Error"));
                converted=converter.GetMethod("DeserializeObject",new[]{typeof(string),typeof(Type),settingsType}).Invoke(null,new object[]{value.GetRawText(),type,settings});
            } else converted=JsonSerializer.Deserialize(value.GetRawText(),type,Options);
            if(type.IsEnum && !Enum.IsDefined(type,converted)) {
                if(!type.IsDefined(typeof(FlagsAttribute),false))throw new ArgumentException("undefined enum value for "+type.Name);
                long mask=0; foreach(var item in Enum.GetValues(type))mask|=Convert.ToInt64(item);
                if((Convert.ToInt64(converted)&~mask)!=0)throw new ArgumentException("undefined enum flags");
            }
            if(converted==null && type.IsValueType && Nullable.GetUnderlyingType(type)==null)throw new ArgumentException("null is not allowed for "+type.Name);
            return converted;
        } catch(ArgumentException){throw;} catch(Exception ex){throw new ArgumentException("invalid "+type.Name+": "+ex.Message,ex);}
    }
    public static object[] BindArguments(MethodInfo method,JsonElement arguments) {
        if(arguments.ValueKind!=JsonValueKind.Object)throw new ArgumentException("arguments must be an object");
        var parameters=method.GetParameters();
        foreach(var input in arguments.EnumerateObject())if(!parameters.Any(p=>p.Name==input.Name))throw new ArgumentException("unknown argument: "+input.Name);
        return parameters.Select(p=>{
            if(p.ParameterType==typeof(CancellationToken))return (object)CancellationToken.None;
            if(!arguments.TryGetProperty(p.Name,out var value)) {
                if(p.HasDefaultValue)return p.DefaultValue;
                throw new ArgumentException("missing argument: "+p.Name);
            }
            return ConvertValue(value,p.ParameterType);
        }).ToArray();
    }
    public record PatchValue(object Target,PropertyInfo Property,object Value);
    public static List<PatchValue> PreparePatch(object target,string[] allowed,JsonElement patch) {
        if(patch.ValueKind!=JsonValueKind.Object)throw new ArgumentException("patch must be an object");
        var changes=new List<PatchValue>();
        foreach(var item in patch.EnumerateObject()) {
            if(!allowed.Contains(item.Name))throw new ArgumentException("unsupported setting: "+item.Name);
            var property=target.GetType().GetProperty(item.Name,BindingFlags.Public|BindingFlags.Instance);
            if(property?.SetMethod?.IsPublic!=true)throw new ArgumentException("setting is not writable in this client version: "+item.Name);
            changes.Add(new PatchValue(target,property,ConvertValue(item.Value,property.PropertyType)));
        }
        return changes;
    }
    public static void CommitPatch(List<PatchValue> changes,Func<object> save,Action<Action> dispatch,TimeSpan timeout,Action rollback=null) {
        object[] old=null; object pending=null;
        try {
            dispatch(()=>{
                old=changes.Select(p=>p.Property.GetValue(p.Target)).ToArray();
                foreach(var p in changes)p.Property.SetValue(p.Target,p.Value);
                pending=save();
            });
            if(pending is Task task) {
                task.WaitAsync(timeout).GetAwaiter().GetResult();
            }
        } catch(TimeoutException){throw;}
        catch {
            dispatch(()=>{if(old!=null)for(int i=0;i<changes.Count;i++)changes[i].Property.SetValue(changes[i].Target,old[i]);rollback?.Invoke();});
            throw;
        }
        dispatch(()=>{
            foreach(var p in changes)if(JsonSerializer.Serialize(p.Property.GetValue(p.Target),Options)!=JsonSerializer.Serialize(p.Value,Options))
                throw new InvalidOperationException("settings readback mismatch for "+p.Property.Name);
        });
    }
}
