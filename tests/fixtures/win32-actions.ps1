param([string]$Title, [string]$Receipt)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName PresentationFramework
$xaml = @'
<Window xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" Width="640" Height="700" WindowStartupLocation="CenterScreen">
  <StackPanel Margin="12">
    <TextBlock Text="xbot-cli isolated UIA test" FontSize="18"/>
    <TextBox x:Name="Input" AutomationProperties.AutomationId="Input" AutomationProperties.Name="Input" Margin="0,8" Height="32"/>
    <TextBox x:Name="ReadOnly" AutomationProperties.AutomationId="ReadOnly" Text="read-only" IsReadOnly="True" Height="28"/>
    <Button x:Name="Invoke" AutomationProperties.AutomationId="Invoke" Content="Invoke" Height="30" Margin="0,6"/>
    <CheckBox x:Name="Check" AutomationProperties.AutomationId="Check" Content="Check"/>
    <ListBox x:Name="List" AutomationProperties.AutomationId="List" Height="65">
      <ListBoxItem AutomationProperties.AutomationId="Alpha" Content="Alpha"/>
      <ListBoxItem AutomationProperties.AutomationId="Beta" Content="Beta"/>
    </ListBox>
    <TreeView Height="70">
      <TreeViewItem x:Name="Branch" AutomationProperties.AutomationId="Branch" Header="Branch">
        <TreeViewItem Header="Leaf"/>
      </TreeViewItem>
    </TreeView>
    <ScrollViewer x:Name="Scroller" AutomationProperties.AutomationId="Scroller" Height="120" VerticalScrollBarVisibility="Auto">
      <StackPanel x:Name="LongContent"/>
    </ScrollViewer>
    <StackPanel x:Name="VictimHost"><Button x:Name="Victim" AutomationProperties.AutomationId="Victim" Content="Victim" Height="28"/></StackPanel>
    <Button x:Name="Replace" AutomationProperties.AutomationId="Replace" Content="Replace victim" Height="28"/>
    <Button x:Name="Disabled" AutomationProperties.AutomationId="Disabled" Content="Disabled" IsEnabled="False"/>
    <TextBlock x:Name="Status" AutomationProperties.AutomationId="Status" Text="0"/>
  </StackPanel>
</Window>
'@
$reader = [System.Xml.XmlNodeReader]::new([xml]$xaml)
$window = [Windows.Markup.XamlReader]::Load($reader)
$window.Title = $Title
$script:count = 0
$script:replaced = 0
$script:references = [System.Collections.Generic.List[object]]::new()
$window.FindName('Invoke').Add_Click({ $script:count++; $window.FindName('Status').Text = [string]$script:count })
$window.FindName('Replace').Add_Click({
    $hostPanel = $window.FindName('VictimHost')
    $script:references.Add($hostPanel.Children[0])
    $hostPanel.Children.Clear()
    $button = [Windows.Controls.Button]::new()
    $button.Content = 'Victim'
    $button.Height = 28
    [Windows.Automation.AutomationProperties]::SetAutomationId($button, 'Victim')
    [void]$hostPanel.Children.Add($button)
    $script:replaced++
})
foreach ($index in 1..60) {
    $text = [Windows.Controls.TextBlock]::new()
    $text.Text = "Scroll row $index"
    $text.Height = 22
    [void]$window.FindName('LongContent').Children.Add($text)
}
$timer = [Windows.Threading.DispatcherTimer]::new()
$timer.Interval = [TimeSpan]::FromMilliseconds(100)
$timer.Add_Tick({
    $record = @{
        pid=$PID; title=$Title; text=$window.FindName('Input').Text
        checked=$window.FindName('Check').IsChecked
        selected=$window.FindName('List').SelectedIndex
        expanded=$window.FindName('Branch').IsExpanded
        scroll=$window.FindName('Scroller').VerticalOffset
        count=$script:count; replaced=$script:replaced
    }
    try {
        $record | ConvertTo-Json -Compress | Set-Content -LiteralPath $Receipt -Encoding UTF8
    } catch {
        # The verifier may be reading while this tick writes. Retry next tick.
    }
})
$window.Add_Closed({ $timer.Stop() })
$timer.Start()
[void]$window.ShowDialog()
