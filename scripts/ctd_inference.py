import numpy as np
import cv2

img = cv2.imread('data/1746025823_segment.png')

kernel = np.ones((3,3),np.uint8)
h, w = img.shape[0], img.shape[1]
seedpnt = (int(w/2), int(h/2))
difres = 10

# convert to grayscale
img = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)

# ballon_mask = img - 127
ballon_mask = 127 - img
ballon_mask = img
ballon_mask = cv2.dilate(ballon_mask, kernel,iterations = 1)
# ballon_area, _, _, rect = cv2.floodFill(ballon_mask, mask=None, seedPoint=seedpnt,  flags=4, newVal=(30), loDiff=(difres, difres, difres), upDiff=(difres, difres, difres))
ballon_mask = 30 - ballon_mask
retval, ballon_mask = cv2.threshold(ballon_mask, 1, 255, cv2.THRESH_BINARY)
ballon_mask = cv2.bitwise_not(ballon_mask, ballon_mask)

# box_kernel = int(np.sqrt(ballon_area) / 30)
# if box_kernel > 1:
#     box_kernel = np.ones((box_kernel,box_kernel),np.uint8)
#     ballon_mask = cv2.dilate(ballon_mask, box_kernel, iterations = 1)
#     ballon_mask = cv2.erode(ballon_mask, box_kernel, iterations = 1)

cv2.imshow('ballon_mask', ballon_mask)
#cv2.imshow('img', img)
cv2.waitKey(0)
