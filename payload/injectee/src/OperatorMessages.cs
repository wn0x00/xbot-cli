using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;

namespace XbotInjectee;

internal static class OperatorMessages
{
    public static object Create(Type type, object action, bool force, string requestName = null, object request = null)
    {
        var ctor = type.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .FirstOrDefault(c => c.GetParameters().Any(p => p.Name == "action") &&
                c.GetParameters().Any(p => p.Name == "force") &&
                (requestName == null || c.GetParameters().Any(p => p.Name == requestName)));
        if (ctor == null) throw new NotSupportedException("ApiOperatorMessage named constructor unavailable");
        var args = ctor.GetParameters().Select(p => p.Name == "action" ? action : p.Name == "force" ? (object)force :
            p.Name == requestName ? request : p.HasDefaultValue ? p.DefaultValue :
            p.ParameterType.IsValueType ? Activator.CreateInstance(p.ParameterType) : null).ToArray();
        return ctor.Invoke(args);
    }
}
