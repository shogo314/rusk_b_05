#!/usr/bin/env python3
"""Run the reference-lifetime cases under AddressSanitizer and UBSan."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = [
    ((ROOT/'examples/beta05/enums.rusk').read_text(),b'',b'5\n5\n8\n'),
    ('''enum Cell<T>{Full(T),Empty}
let text:="abc";let cell:=Cell::Full(text[0]);let alias:=cell;
let held:=match cell{Full(c)=>c,Empty=>'?',};
cell=Cell::Empty;held='Z';wt text;
held:='Q';text.push_back('!');wt held,text;
let rows:Vec<Cell<Vec<Int>>>:=[];
for i in 0..200{rows.push_back(Cell::Full([i]));}
let detached=rows;let saved:=match rows[0]{Full(r)=>r,Empty=>[],};
rows=[];saved[0]=7;wt saved,match detached[0]{Full(r)=>r,Empty=>[],};
''',b'',b'Zbc\nQ Zbc!\n7 0\n'),
    ((ROOT/'examples/beta05/blocks_and_labels.rusk').read_text(),b'',b'a b\nZbc!\n1 2\n'),
    ((ROOT/'examples/beta05/control_flow.rusk').read_text(),b'',b'b c\nab!\nab!? 2\n13\n'),
    ('''let o:=Some([1]);let alias:=o;let held:=o?!;
o=None;held[0]=7;wt alias.is_none(),held;
let d:=Dict::<Str,Vec<Int>>();d.set("a",held);let saved:=d.get("a")?!;
let detached=d;d.erase("a");saved[0]=8;wt saved,detached.get("a")?!.len();
for i in 0..1000{d.set(Str(i),[i]);}d.clear();wt saved;
''',b'',b'Yes 7\n8 1\n8\n'),
    ('''let s:=Set::<(Int,Str)>();let key:=(1,"a");s.insert(key);key[1]="b";
for k in s{k[0]=99;s.insert((1,"a"));}wt s.contains((1,"a"));
let d:=Dict::<Int,Int>();for i in 0..50{d.set(i,i);}
let it:=d.map((p:(Int,Int))=>p[1]);let other=it;
wt it.fold(0,(a:Int,b:Int)=>a+b),other.fold(0,(a:Int,b:Int)=>a+b);
d.clear();s.clear();
''',b'',b'Yes\n1225 1225\n'),
    ('''let text:="abc";let o:=Some(text[0]);let c:=if true{o?!}else{text[1]};
o=None;c='Z';wt text;let independent=c;c:='Q';text.push_back('!');wt independent,text;
in grid(2,2):Vec<Vec<Int>>,pairs(2):Vec<(Int,Str)>;wt grid,pairs;
wt try_read::<Int>().is_none();
''',b'1 2 3 4 5 x 6 y\n',b'Zbc\nZ Zbc!\n1 2 3 4 5 x 6 y\nYes\n'),
    ('''let text:="abc";let c:=text[1];let f:=()=>c;
let independent=f;c:='Q';text:="new";let held:=f();held='Z';
wt f(),independent(),text;
let values:=[1,2,3,4];let n:=0;
let it:=values.filter((x:Int)=>x%2==0).map((x:Int)=>x+(n+=1));
let copy_it=it;wt it.to_vec(),n;wt copy_it.to_vec(),n;
''',b'',b'Z b new\n3 6 2\n3 6 2\n'),
    (f'include {json.dumps(str(ROOT/"examples/beta04/library/priority_queue.rusk"))};\n'
     f'include {json.dumps(str(ROOT/"examples/beta04/library/segment_tree.rusk"))};\n'
     + '''let queue:=PriorityQueue([3,1,2],(a:Int,b:Int)=>a<b);
let detached=queue;wt queue.pop(),queue.pop(),detached.pop();
let seg:=SegTree(["a","b","c"],"",(a:Str,b:Str)=>a+b);
let changed=seg;changed.set(1,"X");wt seg.prod(0,3),changed.prod(0,3);
wt seg.max_right(0,(s:Str)=>s.len()<=2),seg.min_left(3,(s:Str)=>s.len()<=2);
''',b'',b'1 2 1\nabc aXc\n2 1\n'),
    ('''let s:="abc";
{ let c:=s[0]; c='z'; s="XYZ"; wt c; }
s.push_back('!'); wt s;
let a := [[1,2],[3]]; let row:=a[0]; let old:=row[0];
for i in 0..1000 { a.push_back([i]); }
a=[[9]]; row[0]=8; wt old,a;
let copied = a; copied[0][0]=10; wt a,copied;
''', b'', b'X\nXYZ!\n8 9\n9 10\n'),
    ('''fn first() -> Char { let s:="abc"; return s[0]; }
let saved:Vec<Char>:=[];
for i in 0..200 { saved.push_back(first()); }
for c in saved { c='Z'; }
wt saved.len(),saved[0],saved[-1];
let detached = saved; saved=[]; wt detached.len();
let s:="a";
while s[0]=='a' { s.push_back('!'); s[0]='b'; continue; }
for i in 0..3 { let c:=s[0]; if i==1 { break; } continue; }
s.push_back('?'); wt s;
''', b'', b'200 Z Z\n200\nb!?\n'),
    ('''struct Envelope<T> { payload:T; }
let text:="abc";
let envelope:=Envelope::<Char> { payload:text[0] };
let held:=envelope.payload;
envelope.payload:='Z'; held:='Q'; text.push_back('!'); wt text;
let original:=Envelope::<(Str,Vec<Int>)> { payload:("old",[1,2]) };
let old:=original.payload[1][0];
let detached = original; original=Envelope::<(Str,Vec<Int>)> { payload:("new",[9]) };
detached.payload[1][0]=7; wt old,detached.payload,original.payload;
''', b'', b'abc!\n1 old 7 2 new 9\n'),
    (f'include {json.dumps(str(ROOT/"examples/beta02/library/fenwick.rusk"))};\n'
     f'include {json.dumps(str(ROOT/"examples/beta02/library/sum_segment_tree.rusk"))};\n'
     + '''let fw:=Fenwick([1,2,3],0); let detached = fw;
fw.add(1,4); wt fw.prefix_sum(3),detached.prefix_sum(3);
let seg:=SumSegTree(["a","b","c"],""); seg.set(1,"X"); wt seg.prod(0,3);
''', b'', b'10 6\naXc\n'),
    ('''fn fresh(x:Int)->Int { return x*2+1; }
let saved:Vec<Int>:=[];
for i in 0..1000 { saved.push_back(fresh(i)); }
let held:=saved[0]; let other = saved; held=8;
wt saved[0],other[0],saved[-1];
in n:Int,s:Str; wt n,s.len(),s[0],s[-1];
s.push_back('!'); wt s.len();
''', b'7 '+b'x'*131100+b'\n', b'8 1 1999\n7 131100 x x\n131101\n'),
    ((ROOT/'examples/prototype/grid_bfs.rusk').read_text(), b'3 4\n....\n.##.\n....\n', b'5\n'),
]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--no-leaks', action='store_true', help='disable LeakSanitizer for ptrace environments')
    args=parser.parse_args()
    env=os.environ.copy()
    if args.no_leaks:
        env['ASAN_OPTIONS']=env.get('ASAN_OPTIONS','')+':detect_leaks=0'
    with tempfile.TemporaryDirectory(prefix='rusk-sanitizers-') as directory:
        temp=Path(directory)
        for source, data, expected in CASES:
            (temp/'main.rusk').write_text(source)
            subprocess.run([str(ROOT/'target/debug/rusk'),'compile',str(temp/'main.rusk'),'-o',str(temp/'main.cpp')],check=True)
            subprocess.run(['g++','-std=c++20','-O1','-g','-fsanitize=address,undefined','-fno-omit-frame-pointer',str(temp/'main.cpp'),'-o',str(temp/'main')],check=True)
            run=subprocess.run([str(temp/'main')],input=data,capture_output=True,env=env)
            assert run.returncode == 0, run.stderr.decode(errors="replace")
            assert run.stdout==expected, (run.stdout,expected)
            assert not run.stderr, run.stderr
    print(f'AddressSanitizer / UBSan: {len(CASES)} cases passed')

if __name__ == '__main__':
    main()
