"""Offline regression for provider nodes with no UIA RuntimeId."""
import importlib.util
import sys
from pathlib import Path
from types import SimpleNamespace
import unittest

sys.dont_write_bytecode=True

spec=importlib.util.spec_from_file_location('snapshot',Path(__file__).resolve().parents[1]/'skills/xbot-activity-fix/scripts/win32/snapshot.py')
snapshot=importlib.util.module_from_spec(spec)
spec.loader.exec_module(snapshot)

class Control:
    IsPassword=False
    Name='owned fixture'
    ControlTypeName='PaneControl'
    AutomationId=''
    ClassName=''
    FrameworkId=''
    ProcessId=1
    NativeWindowHandle=0
    IsEnabled=True
    IsOffscreen=False
    BoundingRectangle=SimpleNamespace(left=0,top=0,right=10,bottom=10)
    def __init__(self,runtime_id,children=()):
        self.runtime_id=runtime_id
        self.children=list(children)
        self.sibling=None
        for current,next_item in zip(self.children,self.children[1:]): current.sibling=next_item
    def GetRuntimeId(self): return self.runtime_id
    def GetFirstChildControl(self): return self.children[0] if self.children else None
    def GetNextSiblingControl(self): return self.sibling

class CollectTests(unittest.TestCase):
    def test_empty_ids_keep_distinct_nodes_and_descendants(self):
        root=Control([42,1],[Control([], [Control([])]),Control([]),Control(None)])
        result=snapshot.collect(root,100,20,1,lambda:True)
        self.assertEqual(result['node_count'],5)
        self.assertTrue(result['traversal_complete'])
        self.assertIn('MISSING_RUNTIME_ID',result['observation_warnings'])
        self.assertEqual([n['runtime_id'] for n in result['nodes'][1:]],[None]*4)
    def test_real_duplicate_ids_still_stop_and_report_incomplete(self):
        root=Control([42,1],[Control([42,2]),Control([42,2])])
        result=snapshot.collect(root,100,20,1,lambda:True)
        self.assertEqual(result['node_count'],2)
        self.assertIn('repeated_runtime_id',result['partial_reasons'])
    def test_missing_id_cycles_are_bounded(self):
        root=Control([])
        root.children=[root]
        result=snapshot.collect(root,100,3,1,lambda:True)
        self.assertEqual(result['node_count'],4)
        self.assertIn('depth_budget',result['partial_reasons'])

if __name__=='__main__': unittest.main()
