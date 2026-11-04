using System;
using Caliburn.Micro;
using ShadowBot.Components.Protocol.Engine;

namespace XbotInjectee;

/// <summary>
/// Bridges to ShadowBot's running container via Caliburn.Micro's static IoC.
/// The injectee runs inside Shell's process; once we're in Default ALC, IoC.Get
/// resolves the same singletons Shell uses.
/// </summary>
internal static class Resolver
{
    public static IEngineComponent Engine()
    {
        try { return IoC.Get<IEngineComponent>(); }
        catch (Exception ex)
        {
            BootstrapLog.Error("Resolver.Engine: " + ex.Message);
            return null;
        }
    }
}
