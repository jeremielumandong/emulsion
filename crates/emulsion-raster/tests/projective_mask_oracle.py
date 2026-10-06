"""Independent exact-dyadic mask oracle and bounded construction-error search.

No packages or application code are loaded. Fraction is the authority. The
float helpers reproduce the rounded candidate geometry solely to exhibit why
runtime-only bounds on a composed inverse do not certify authored C.
Run: python3 crates/emulsion-raster/tests/projective_mask_oracle.py
"""
from fractions import Fraction as F
import math
import struct


def canonical(c):
    pivot = max(c, key=abs)
    return [v / pivot if v else 0.0 for v in c]


def multiply(a, b):
    # Correctly rounded exact dot; stronger than an ordinary naive dot and
    # independent from the sampler's compensated/exact-expansion algorithms.
    return canonical([float(sum((F(a[3*r+k])*F(b[3*k+c]) for k in range(3)), F()))
                      for r in range(3) for c in range(3)])


def inverse(c):
    rows = [max(abs(x) for x in c[3*r:3*r+3]) for r in range(3)]
    balanced = [x / rows[i//3] for i,x in enumerate(c)]
    cols = [max(abs(balanced[3*r+k]) for r in range(3)) for k in range(3)]
    balanced = [v/cols[i%3] for i,v in enumerate(balanced)]
    aug = [balanced[3*r:3*r+3] + [float(r==k) for k in range(3)] for r in range(3)]
    for k in range(3):
        p = max(range(k,3), key=lambda r:abs(aug[r][k]))
        assert abs(aug[p][k]) > 64*2**-52
        aug[k], aug[p] = aug[p], aug[k]
        d=aug[k][k]; aug[k] = [v/d for v in aug[k]]
        for r in range(3):
            if r != k:
                f=aug[r][k]; aug[r]=[a-f*b for a,b in zip(aug[r],aug[k])]; aug[r][k]=0.
    inv=[x for row in aug for x in row[3:]]
    check_residual(balanced, inv)
    # Mirror Scaled arithmetic without overflow. This search uses modest
    # exponents, so normal float operations reproduce each fraction operation.
    scaled=[]
    for i,v in enumerate(inv):
        if not v: scaled.append((0.,0)); continue
        frac,exponent=math.frexp(v); frac*=2; exponent-=1
        for divisor in [cols[i//3],rows[i%3]]:
            f,e=math.frexp(divisor);f*=2;e-=1
            frac/=f; exponent-=e
            ff,ee=math.frexp(frac);frac=ff*2;exponent+=ee-1
        scaled.append((frac,exponent))
    pf,pe=max(scaled,key=lambda p: (p[1],abs(p[0])) if p[0] else (-10000,0))
    result = canonical([math.ldexp(f/pf,e-pe) if f else 0. for f,e in scaled])
    recovered = canonical([v*cols[i//3]*rows[i%3] for i,v in enumerate(result)])
    pivot = max(range(9), key=lambda i:abs(inv[i]))
    scale = inv[pivot]/recovered[pivot]
    check_residual(balanced, [v*scale for v in recovered])
    return result


def check_residual(a,b):
    for left,right in [(a,b),(b,a)]:
        for r in range(3):
            for c in range(3):
                exact_dot = sum((F(left[3*r+k])*F(right[3*k+c]) for k in range(3)),F())
                assert abs(float(exact_dot)-float(r==c)) <= 1e-10


def adjugate(c):
    c=list(map(F,c))
    indices=[(4,8,5,7),(2,7,1,8),(1,5,2,4),(5,6,3,8),(0,8,2,6),(2,3,0,5),(3,7,4,6),(1,6,0,7),(0,4,1,3)]
    return [c[a]*c[b]-c[d]*c[e] for a,b,d,e in indices]


def exact(c, origin, pixel=(0,0)):
    a=adjugate(c);p=[F(origin[i]+pixel[i])+F(1,2) for i in range(2)]+[F(1)]
    n=[sum((a[3*r+k]*p[k] for k in range(3)),F()) for r in range(3)]
    return None if not n[2] else (n[0]/n[2],n[1]/n[2])


def evaluate(c):
    v=[sum((F(c[3*r+k])*p for k,p in enumerate([F(1,2),F(1,2),F(1)])),F()) for r in range(3)]
    return (v[0]/v[2],v[1]/v[2])


def side(value):
    return 'detail' if -F(1,2) < value < F(33,2) else 'exterior'


def search():
    found={}
    # Finite, deterministic search. Nonzero g supplies actual perspective;
    # c's normalized stored coefficients, not pre-normalization values, define
    # the exact inverse reference. Q has a rounded translation basis.
    for origin in [1_000_000_007, 2_000_000_011, -1_000_000_007, -2_000_000_011]:
        for g in [0.,2**-24,-2**-24,2**-30,-2**-30]:
            for ulps in range(-12,13):
                a=2.+ulps*2**-51
                t=origin+0.5-16.5*a+16.5*g*(origin+0.5)
                c=canonical([a,0.,t,0.,1.,0.,g,0.,1.])
                ex=exact(c,(origin,0))
                q=multiply(canonical([1.,0.,-float(origin),0.,1.,0.,0.,0.,1.]),c)
                inv=inverse(q); runtime=evaluate(inv)
                if side(ex[0]) != side(runtime[0]) and abs(float(runtime[0])-16.5)>1e-11:
                    kind=(side(ex[0]),side(runtime[0]))
                    if kind not in found:
                        found[kind]=(c,origin,ex,runtime,inv)
                        if len(found)==2: return found
    return found


def main():
    c=[1.,0.,0.,0.,1.,0.,-0.125,0.,1.]
    assert exact(c,(0,0))==(F(8,17),F(8,17))
    assert exact(c,(0,0),(3,3))==(F(56,23),F(56,23))
    print('cancellation:', exact(c,(0,0)), exact(c,(0,0),(3,3)))
    found=search()
    assert len(found)==2,found
    for direction,(c,origin,ex,runtime,inv) in found.items():
        inverse(c)  # Also passes the checked representation/residual gate.
        print('\nclassification authored -> rounded:',direction,'origin:',origin)
        print('C bits:',[hex(struct.unpack('>Q',struct.pack('>d',v))[0]) for v in c])
        print('C:',repr(c))
        print('authored x =',ex[0],'; float:',float(ex[0]))
        print('authored y =',ex[1],'; float:',float(ex[1]))
        print('rounded inverse x:',float(runtime[0]),'distance from edge:',float(runtime[0]-F(33,2)))
        # Independent runtime-only enclosure around the rounded inverse has
        # no construction error, so it would confidently approve this side.
        print('rounded inverse:',repr(inv))

if __name__=='__main__': main()
