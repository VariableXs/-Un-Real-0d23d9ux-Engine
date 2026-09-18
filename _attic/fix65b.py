import io
p = 'kernel/varix/src/security/kaesgcm.rs'
s = io.open(p, encoding='utf-8').read()

# ① GHASH 尾块清零（feed 整块 hash 后 block 保留旧数据）
old = """        feed(aad, &mut y, &mut block, &mut fill);
        if fill != 0 {
            ghash_block(&self.h, &mut y, &block);
            fill = 0;
            block = [0u8; 16];
        }
        feed(ct, &mut y, &mut block, &mut fill);
        if fill != 0 {
            ghash_block(&self.h, &mut y, &block);
        }"""
new = """        feed(aad, &mut y, &mut block, &mut fill);
        if fill != 0 {
            // 尾块：fill 之后必清零——feed 整块 hash 后 block 保留上一块
            // 旧字节，直接 hash 会把脏数据混进认证面（TC15/16 向量锁定处）。
            for b in block[fill..].iter_mut() {
                *b = 0;
            }
            ghash_block(&self.h, &mut y, &block);
            fill = 0;
            block = [0u8; 16];
        }
        feed(ct, &mut y, &mut block, &mut fill);
        if fill != 0 {
            for b in block[fill..].iter_mut() {
                *b = 0;
            }
            ghash_block(&self.h, &mut y, &block);
        }"""
assert old in s, 'ghash tail not found'
s = s.replace(old, new, 1)

# ② TC15 expect_ct 修正（60B，去掉误加的尾部）
marker = '8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662898015ad'
assert marker in s, 'tc15 marker not found'
s = s.replace(marker, '8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662', 1)

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('kaesgcm fix OK')
