import importlib.util,json,subprocess,tempfile,unittest
from pathlib import Path
P=Path(__file__).with_name('ui_tree_inspect.py')
spec=importlib.util.spec_from_file_location('tree',P);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class TreeTests(unittest.TestCase):
 def test_nested_attributes_and_repeated_ids(self):
  data={'attributes':{'type':'Column','id':'same'},'children':[{'attributes':{'type':'Text','id':'same','text':'你好\n世界','clickable':'false'}},{'attributes':{'type':'Button','bounds':'[1,2][3,4]'}}]}
  roots=m.read_nodes(data);self.assertEqual(m.summarize(roots)['node_count'],3)
  text=m.render_tree(roots);self.assertIn('├─ Text',text);self.assertIn('└─ Button',text);self.assertIn('clickable="false"',text);self.assertIn('你好\\n世界',text)
 def test_window_wrappers_and_metadata_not_nodes(self):
  roots=m.read_nodes({'metadata':{'type':'not-a-ui-node'},'windows':[{'root':{'children':[{'type':'Text','text':'A'}]}},{'root':{'type':'Button'}}]})
  self.assertEqual(m.summarize(roots),{'node_count':2,'by_type':{'Button':1,'Text':1},'max_depth':0,'root_count':2})
 def test_component_type_and_hierarchy(self):
  roots=m.read_nodes({'hierarchy':{'componentType':'Row','children':[{'type':'Text'}]}})
  self.assertEqual(roots[0]['children'][0]['type'],'Text');self.assertEqual(m.summarize(roots)['max_depth'],1)
 def test_empty_or_unknown_schema(self):
  for obj in ({'error':'denied'},{'unexpected':{'type':'Button'}},[],{}):self.assertEqual(m.read_nodes(obj),[])
 def test_cli_outputs_and_failures(self):
  import sys
  with tempfile.TemporaryDirectory(prefix='harmony-tree-test-') as d:
   p=Path(d)/'tree.json';p.write_text(json.dumps({'type':'Column','children':[{'type':'Text','text':'中文'}]}))
   run=subprocess.run([sys.executable,str(P),str(p),'--format','json'],capture_output=True,text=True)
   self.assertEqual(run.returncode,0);self.assertEqual(json.loads(run.stdout)['tree'][0]['children'][0]['attributes']['text'],'中文')
   for content in ('not json','{}'):
    p.write_text(content);run=subprocess.run([sys.executable,str(P),str(p)],capture_output=True,text=True)
    self.assertEqual(run.returncode,2);self.assertEqual(run.stdout,'')
if __name__=='__main__':unittest.main(verbosity=2)
