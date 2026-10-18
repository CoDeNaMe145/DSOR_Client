/// dSFMT-19937, the native item-quality stream (2 - generated [1,2) double).
pub(crate) struct Rolls { state: [u64;384], index:usize }
impl Rolls {
    pub(crate) fn new(seed:u32)->Self {
        let mut words=[0u32;768];words[0]=seed;
        for i in 1..768 {words[i]=1812433253u32.wrapping_mul(words[i-1]^(words[i-1]>>30)).wrapping_add(i as u32);}
        let mut state=[0u64;384];
        for i in 0..384 {state[i]=words[i*2] as u64 | ((words[i*2+1] as u64)<<32);}
        for s in &mut state[..382] {*s=(*s&0x000fffffffffffff)|0x3ff0000000000000;}
        let certificate=((state[382]^0x90014964b32f4329)&0x3d84e1ac0dc82880)^(state[383]&1);
        if certificate.count_ones()%2==0 {state[383]^=1;}
        Self{state,index:382}
    }
    pub(crate) fn next(&mut self )->f32 {
        if self.index>=382 {
            let mut lung=[self.state[382],self.state[383]];
            for i in 0..191 {
                let j=(i+117)%191;
                let a=[self.state[i*2],self.state[i*2+1]];
                lung=[(a[0]<<19)^lung[1].rotate_left(32)^self.state[j*2],(a[1]<<19)^lung[0].rotate_left(32)^self.state[j*2+1]];
                self.state[i*2]=a[0]^(lung[0]>>12)^(lung[0]&0x000ffafffffffb3f);
                self.state[i*2+1]=a[1]^(lung[1]>>12)^(lung[1]&0x000ffdfffc90fffd);
            }
            self.state[382]=lung[0];self.state[383]=lung[1];self.index=0;
        }
        let value=(2.0-f64::from_bits(self.state[self.index])) as f32;self.index+=1;value
    }
}
